//! The iterative passes on tiny float targets in a captured frame (L113):
//!
//!     cargo run --release -p d3d9frame --example sim_loops -- capture.ddframe [frame] [--shaders]
//!
//! A "sim draw" draws into a texture-backed A32B32G32R32F target of at most
//! 64x64. Consecutive sim draws form a loop. For each loop of the frame
//! (default: the third frame, the first ones may be partial) it prints the
//! iterations (targets, sampled textures, shaders, constants that changed),
//! where the loop's inputs come from (uploads, earlier draws) and which
//! later draws sample its outputs. `--shaders` disassembles each distinct
//! shader the loops use, with its constant table names.
use d3d9cmd::{Constants, DrawCall, Upload};
use d3d9frame::{Command, Reader};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

const A32B32G32R32F: u32 = 116;

fn is_sim(d: &DrawCall) -> bool {
    d.targets[0].as_ref().is_some_and(|t| t.format == A32B32G32R32F && t.texture.is_some() && t.width <= 64 && t.height <= 64)
}

fn shader_id(ids: &mut HashMap<Vec<u32>, usize>, s: &Option<Arc<[u32]>>) -> String {
    match s {
        None => "-".into(),
        Some(t) => {
            let n = ids.len();
            format!("#{}", ids.entry(t.to_vec()).or_insert(n))
        }
    }
}

/// The float registers whose bits differ between two constant blocks.
fn changed(a: &Constants, b: &Constants) -> Vec<usize> {
    let n = a.f.len().max(b.f.len());
    (0..n).filter(|&k| a.f.get(k) != b.f.get(k)).collect()
}

fn floats(c: &Constants, k: usize) -> String {
    c.f.get(k).map_or("-".into(), |v| format!("[{}]", v.iter().map(|x| format!("{:.4}", f32::from_bits(*x))).collect::<Vec<_>>().join(" ")))
}

fn targets(d: &DrawCall) -> String {
    d.targets
        .iter()
        .enumerate()
        .filter_map(|(k, t)| t.as_ref().map(|t| format!("RT{k}=tex{:#x}/{}x{}", t.texture.map_or(0, |x| x.resource), t.width, t.height)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn sampled(d: &DrawCall) -> String {
    d.textures
        .iter()
        .map(|t| format!("s{}=tex{:#x}({}x{} f{})", t.sampler, t.resource, t.width, t.height, t.format))
        .collect::<Vec<_>>()
        .join(" ")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("capture path");
    let want: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(2);
    let show_shaders = args.iter().any(|a| a == "--shaders");
    let reader = Reader::open(path).expect("open capture");
    if args.iter().any(|a| a == "--count") {
        // commands per frame by kind (frames end at Present)
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let mut frame = 0;
        for c in reader {
            let c = match c {
                Ok(c) => c,
                Err(e) => {
                    println!("read error after frame {frame}: {e}");
                    break;
                }
            };
            *counts.entry(c.kind()).or_default() += 1;
            if let Command::Present(_) = c {
                println!("frame {frame}: {counts:?}");
                counts.clear();
                frame += 1;
            }
        }
        println!("end: frame {frame} partial {counts:?}");
        return;
    }

    let mut frame = 0usize;
    let mut ids: HashMap<Vec<u32>, usize> = HashMap::new();
    // who last wrote each texture resource: an upload or a draw (kept across frames)
    let mut writer: HashMap<u32, String> = HashMap::new();
    let mut draws = 0usize;
    let mut loops: Vec<Vec<(usize, DrawCall)>> = Vec::new();
    let mut in_loop = false;
    let mut outputs: BTreeSet<u32> = BTreeSet::new();
    // consumers: (draw index, sampler, resource, vs, ps, targets)
    let mut consumers: Vec<String> = Vec::new();
    let mut loop_inputs: Vec<BTreeMap<u32, String>> = Vec::new();
    for c in reader {
        let c = c.expect("read");
        if frame != want {
            if let Command::Present(_) = c {
                frame += 1;
            }
            // remember uploads before the frame too
            if let Command::Upload(Upload::Image { resource, .. }) = &c {
                writer.insert(*resource, format!("upload (frame {frame})"));
            }
            if let Command::Draw(d) = &c {
                for t in d.targets.iter().flatten() {
                    if let Some(x) = t.texture {
                        writer.insert(x.resource, format!("draw in frame {frame}"));
                    }
                }
            }
            continue;
        }
        match &c {
            Command::Present(_) => break,
            Command::Upload(Upload::Image { resource, .. }) => {
                writer.insert(*resource, "upload".into());
            }
            Command::Draw(d) => {
                let n = draws;
                draws += 1;
                if is_sim(d) {
                    if !in_loop {
                        loops.push(Vec::new());
                        loop_inputs.push(BTreeMap::new());
                        in_loop = true;
                    }
                    let li = loops.len() - 1;
                    for t in d.textures.iter() {
                        if !outputs.contains(&t.resource) || !loops[li].iter().any(|(_, p)| p.targets.iter().flatten().any(|x| x.texture.is_some_and(|y| y.resource == t.resource))) {
                            loop_inputs[li].entry(t.resource).or_insert_with(|| writer.get(&t.resource).cloned().unwrap_or_else(|| "unknown (before capture)".into()));
                        }
                    }
                    loops[li].push((n, d.clone()));
                    for t in d.targets.iter().flatten() {
                        if let Some(x) = t.texture {
                            outputs.insert(x.resource);
                        }
                    }
                } else {
                    in_loop = false;
                    for t in d.textures.iter() {
                        if outputs.contains(&t.resource) {
                            consumers.push(format!(
                                "draw {n}: samples tex{:#x} at s{} ({}); vs {} ps {}; {} prims; writes {}",
                                t.resource,
                                t.sampler,
                                if t.sampler >= 257 { "vertex texture" } else { "pixel" },
                                shader_id(&mut ids, &d.vertex_shader),
                                shader_id(&mut ids, &d.pixel_shader),
                                d.primitives,
                                targets(d)
                            ));
                        }
                    }
                }
                for t in d.targets.iter().flatten() {
                    if let Some(x) = t.texture {
                        writer.insert(x.resource, format!("draw {n}"));
                    }
                }
            }
            Command::Clear(cl) if cl.targets.iter().flatten().any(|t| t.format == A32B32G32R32F && t.width <= 64) => {
                in_loop = false;
            }
            Command::Copy(_) | Command::Fill(_) | Command::Clear(_) => in_loop = false,
            _ => {}
        }
    }
    println!("frame {want}: {draws} draws, {} sim loops, {} sim draws", loops.len(), loops.iter().map(Vec::len).sum::<usize>());
    let mut shapes: BTreeMap<String, usize> = BTreeMap::new();
    for l in &loops {
        let key = format!("{} draws, first {}", l.len(), targets(&l[0].1));
        *shapes.entry(key).or_default() += 1;
    }
    // bodies: a body starts at a two-target draw after a one-target draw
    let mut bodies: BTreeMap<String, usize> = BTreeMap::new();
    for l in &loops {
        let mut start = 0;
        for k in 1..=l.len() {
            let starts = k == l.len() || l[k].1.targets[1].is_some() && l[k - 1].1.targets[1].is_none();
            if starts {
                let b = &l[start..k];
                let size = b[0].1.targets[0].as_ref().map_or((0, 0), |t| (t.width, t.height));
                *bodies.entry(format!("{}x{} body, {} passes", size.0, size.1, b.len())).or_default() += 1;
                start = k;
            }
        }
    }
    println!("bodies:");
    for (k, v) in &bodies {
        println!("  {v} x {k}");
    }
    println!("loop shapes:");
    for (k, v) in &shapes {
        println!("  {v} x {k}");
    }
    for (li, l) in loops.iter().enumerate().take(3) {
        println!("\nLOOP {li}: {} draws (draw {}..={}); inputs:", l.len(), l[0].0, l.last().unwrap().0);
        for (r, w) in &loop_inputs[li] {
            println!("  tex{r:#x} from {w}");
        }
        let mut prev: Option<&DrawCall> = None;
        for (n, d) in l {
            let vs = shader_id(&mut ids, &d.vertex_shader);
            let ps = shader_id(&mut ids, &d.pixel_shader);
            let (cv, cp) = prev.map_or((vec![], vec![]), |p| (changed(&p.vs_constants, &d.vs_constants), changed(&p.ps_constants, &d.ps_constants)));
            println!(
                "  draw {n}: prim {} x{} verts {} {} | {} | vs {vs} ps {ps} | vs c changed {:?} ps c changed {:?} | vp {:?}",
                d.primitive, d.primitives, d.vertex_count, targets(d), sampled(d), cv, cp, &d.viewport[..4]
            );
            if prev.is_none() || li == 0 && *n == l[0].0 {
                for k in 0..d.ps_constants.f.len().min(16) {
                    println!("      ps c{k} = {}", floats(&d.ps_constants, k));
                }
            }
            for k in &cp {
                if *k < 64 {
                    println!("      ps c{k}: {}", floats(&d.ps_constants, *k));
                }
            }
            prev = Some(d);
        }
    }
    println!("\nconsumers of loop outputs ({}):", consumers.len());
    for c in consumers.iter().take(40) {
        println!("  {c}");
    }
    if show_shaders {
        let mut used: BTreeSet<usize> = BTreeSet::new();
        for l in &loops {
            for (_, d) in l {
                for s in [&d.vertex_shader, &d.pixel_shader] {
                    if let Some(t) = s {
                        used.insert(ids[&t.to_vec()]);
                    }
                }
            }
        }
        for (tokens, id) in &ids {
            if !used.contains(id) {
                continue;
            }
            println!("\nSHADER #{id} ({} tokens)", tokens.len());
            match d3d9shader::parse(tokens) {
                Ok(s) => {
                    if let Some(t) = d3d9shader::ctab::table(&s) {
                        println!("  creator {:?}", t.creator);
                        for c in &t.constants {
                            println!("  const {} set {} reg {} count {}", c.name, c.register_set, c.register_index, c.register_count);
                        }
                    }
                    for line in d3d9shader::disasm::disassemble(&s) {
                        println!("  {line}");
                    }
                }
                Err(e) => println!("  parse error {e:?}"),
            }
        }
    }
}
