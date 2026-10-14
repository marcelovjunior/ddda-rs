//! CPU-only command-stream profiler. See docs/FRAME_ORACLE.md for semantics.
use d3d9cmd::{Constants, DrawCall, Target};
use d3d9frame::{Command, Reader};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufWriter, Read, Write};
use std::ops::Range;

type Histogram = BTreeMap<u64, u64>;
type ShaderPair = (Option<Vec<u32>>, Option<Vec<u32>>);
const GROUPS: [&str; 10] = [
    "shaders",
    "declaration/fvf",
    "render_states",
    "textures/samplers",
    "vertex_streams",
    "index_buffer",
    "vs_constants",
    "ps_constants",
    "viewport/scissor",
    "targets",
];

#[derive(Default, Serialize)]
struct Totals {
    frames: u64,
    commands: u64,
    passes: u64,
    draws: u64,
    primitives: u64,
    comparisons: u64,
    changed: [u64; 10],
    vs_float4_changed: u64,
    ps_float4_changed: u64,
    vs_bytes_changed: u64,
    ps_bytes_changed: u64,
    operations: BTreeMap<String, u64>,
    operation_pixel_visits: BTreeMap<String, u64>,
    runs_same_constants: Histogram,
    runs_ignoring_constants: Histogram,
}

impl Totals {
    fn add(&mut self, other: &Self) {
        self.frames += other.frames;
        self.commands += other.commands;
        self.passes += other.passes;
        self.draws += other.draws;
        self.primitives += other.primitives;
        self.comparisons += other.comparisons;
        for (a, b) in self.changed.iter_mut().zip(other.changed) {
            *a += b;
        }
        self.vs_float4_changed += other.vs_float4_changed;
        self.ps_float4_changed += other.ps_float4_changed;
        self.vs_bytes_changed += other.vs_bytes_changed;
        self.ps_bytes_changed += other.ps_bytes_changed;
        for (k, v) in &other.operations {
            *self.operations.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.operation_pixel_visits {
            *self.operation_pixel_visits.entry(k.clone()).or_default() += v;
        }
        for (k, v) in &other.runs_same_constants {
            *self.runs_same_constants.entry(*k).or_default() += v;
        }
        for (k, v) in &other.runs_ignoring_constants {
            *self.runs_ignoring_constants.entry(*k).or_default() += v;
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
struct Reads {
    /// Distinct static external float4 registers, including matrix rows,
    /// excluding DEF locals. Not a count of dynamically executed reads.
    static_float4: Option<usize>,
    relative: bool,
    status: &'static str,
}

fn shader_reads(tokens: Option<&[u32]>) -> Reads {
    use d3d9shader::{op, reg};
    let Some(tokens) = tokens else {
        return Reads {
            static_float4: None,
            relative: false,
            status: "fixed_function",
        };
    };
    let Ok(shader) = d3d9shader::parse(tokens) else {
        return Reads {
            static_float4: None,
            relative: false,
            status: "parse_error",
        };
    };
    let index = |kind, n| match kind {
        reg::CONST => Some(n),
        reg::CONST2 => Some(n + 2048),
        reg::CONST3 => Some(n + 4096),
        reg::CONST4 => Some(n + 6144),
        _ => None,
    };
    let mut locals = BTreeSet::new();
    let mut reads = BTreeSet::new();
    let mut relative = false;
    for inst in shader.instructions {
        if inst.opcode == op::DEF {
            if let Some(p) = inst.dst {
                if let Some(n) = index(p.kind, p.index) {
                    locals.insert(n);
                }
            }
        }
        for (slot, p) in inst.src.iter().enumerate() {
            if let Some(n) = index(p.kind, p.index) {
                relative |= p.relative.is_some();
                let rows = if slot == 1 {
                    match inst.opcode {
                        op::M4X4 | op::M3X4 => 4,
                        op::M4X3 | op::M3X3 => 3,
                        op::M3X2 => 2,
                        _ => 1,
                    }
                } else {
                    1
                };
                reads.extend(n..n + rows);
            }
        }
    }
    Reads {
        static_float4: Some(reads.difference(&locals).count()),
        relative,
        status: if relative {
            "static_bases_only_dynamic_extent_unknown"
        } else {
            "static_references_not_runtime_execution"
        },
    }
}

// Constants contain raw bits, so NaNs and signed zero compare exactly.
// Missing float registers are zero, matching the capture/backend convention.
fn delta(a: &Constants, b: &Constants) -> (u64, u64) {
    let floats = (0..a.f.len().max(b.f.len()))
        .filter(|&i| a.f.get(i).unwrap_or(&[0; 4]) != b.f.get(i).unwrap_or(&[0; 4]))
        .count() as u64;
    let ints = a.i.iter().zip(&b.i).filter(|(a, b)| a != b).count() as u64;
    let bools = a.b.iter().zip(&b.b).filter(|(a, b)| a != b).count() as u64;
    (floats, floats * 16 + ints * 16 + bools * 4)
}

fn changes(a: &DrawCall, b: &DrawCall) -> [bool; 10] {
    [
        a.vertex_shader != b.vertex_shader || a.pixel_shader != b.pixel_shader,
        a.declaration != b.declaration || a.fvf != b.fvf,
        a.render_states != b.render_states,
        a.textures != b.textures,
        a.vertices != b.vertices,
        a.indices != b.indices,
        delta(&a.vs_constants, &b.vs_constants).1 != 0,
        delta(&a.ps_constants, &b.ps_constants).1 != 0,
        a.viewport != b.viewport || a.scissor != b.scissor,
        a.targets != b.targets || a.depth != b.depth,
    ]
}

fn batch_equal(a: &DrawCall, b: &DrawCall, ignore_constants: bool) -> bool {
    // Only start/base_vertex/min_index/vertex_count/primitives may differ.
    // Inline data and stream offsets remain significant. This is a state
    // compatibility candidate, not proof that geometry can be merged.
    a.primitive == b.primitive
        && changes(a, b)
            .iter()
            .enumerate()
            .all(|(i, changed)| !changed || (ignore_constants && (i == 6 || i == 7)))
}

fn format_name(format: u32) -> String {
    // D3DFORMAT numeric values, as used in d3d9cmd and frame image decoding.
    match format {
        21 => "A8R8G8B8",
        22 => "X8R8G8B8",
        23 => "R5G6B5",
        28 => "A8",
        32 => "A8B8G8R8",
        75 => "D24S8",
        77 => "D24X8",
        80 => "D16",
        111 => "R16F",
        112 => "G16R16F",
        113 => "A16B16G16R16F",
        114 => "R32F",
        115 => "G32R32F",
        116 => "A32B32G32R32F",
        _ => return format!("D3DFMT({format})"),
    }
    .to_owned()
}

fn target_json(t: &Target) -> Value {
    json!({"surface":t.surface, "format":t.format, "format_name":format_name(t.format),
        "width":t.width, "height":t.height, "level":t.level, "samples":t.samples,
        "quality":t.quality, "texture":t.texture.map(|x| json!({"resource":x.resource,
            "kind":format!("{:?}", x.kind), "width":x.width, "height":x.height,
            "depth":x.depth, "levels":x.levels, "usage":x.usage}))})
}

fn target_label(t: &Target) -> String {
    format!(
        "{}x{} {} @{:x}/L{} MS{}:Q{}",
        t.width,
        t.height,
        format_name(t.format),
        t.surface,
        t.level,
        t.samples,
        t.quality
    )
}

#[derive(Serialize)]
struct DrawStats {
    command: u64,
    draw: u64,
    /// None on first draw of frame: no predecessor, not a measured change.
    changed: Option<[bool; 10]>,
    vs_float4_changed: Option<u64>,
    ps_float4_changed: Option<u64>,
    vs_bytes_changed: Option<u64>,
    ps_bytes_changed: Option<u64>,
    vs_reads: Reads,
    ps_reads: Reads,
}

#[derive(Serialize)]
struct Pass {
    ordinal: usize,
    first_command: u64,
    last_command: u64,
    name: String,
    targets: Vec<Value>,
    depth: Option<Value>,
    totals: Totals,
    distinct_shader_pairs: usize,
    distinct_texture_sets: usize,
    distinct_render_state_lists: usize,
    draws: Vec<DrawStats>,
    #[serde(skip)]
    key: ([Option<Target>; 4], Option<Target>),
    #[serde(skip)]
    shaders: BTreeSet<ShaderPair>,
    #[serde(skip)]
    textures: BTreeSet<String>,
    #[serde(skip)]
    states: BTreeSet<Vec<(u32, u32)>>,
    #[serde(skip)]
    runs: [u64; 2],
}

impl Pass {
    fn new(ordinal: usize, command: u64, targets: &[Option<Target>; 4], depth: &Option<Target>) -> Self {
        let colours: Vec<_> = targets
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.as_ref().map(|t| format!("RT{i} {}", target_label(t))))
            .collect();
        let name = format!(
            "{} + depth {}",
            if colours.is_empty() {
                "no colour".into()
            } else {
                colours.join(", ")
            },
            depth.as_ref().map(target_label).unwrap_or_else(|| "none".into())
        );
        Self {
            ordinal,
            first_command: command,
            last_command: command,
            name,
            targets: targets
                .iter()
                .map(|t| t.as_ref().map(target_json).unwrap_or(Value::Null))
                .collect(),
            depth: depth.as_ref().map(target_json),
            totals: Totals::default(),
            distinct_shader_pairs: 0,
            distinct_texture_sets: 0,
            distinct_render_state_lists: 0,
            draws: vec![],
            key: (targets.clone(), depth.clone()),
            shaders: BTreeSet::new(),
            textures: BTreeSet::new(),
            states: BTreeSet::new(),
            runs: [0; 2],
        }
    }
    fn break_runs(&mut self) {
        if self.runs[0] > 0 {
            *self.totals.runs_same_constants.entry(self.runs[0]).or_default() += 1;
        }
        if self.runs[1] > 0 {
            *self
                .totals
                .runs_ignoring_constants
                .entry(self.runs[1])
                .or_default() += 1;
        }
        self.runs = [0; 2];
    }
    fn finish(&mut self) {
        self.break_runs();
        self.distinct_shader_pairs = self.shaders.len();
        self.distinct_texture_sets = self.textures.len();
        self.distinct_render_state_lists = self.states.len();
        self.shaders.clear();
        self.textures.clear();
        self.states.clear();
    }
}

#[derive(Serialize)]
struct Frame {
    frame: u64,
    complete: bool,
    totals: Totals,
    passes: Vec<Pass>,
    events: Vec<Value>,
    #[serde(skip)]
    previous: Option<DrawCall>,
    #[serde(skip)]
    shader_cache: BTreeMap<Vec<u32>, Reads>,
}

impl Frame {
    fn new(frame: u64) -> Self {
        Self {
            frame,
            complete: false,
            totals: Totals::default(),
            passes: vec![],
            events: vec![],
            previous: None,
            shader_cache: BTreeMap::new(),
        }
    }
    fn reads(&mut self, tokens: Option<&[u32]>) -> Reads {
        let Some(tokens) = tokens else {
            return shader_reads(None);
        };
        self.shader_cache
            .entry(tokens.to_vec())
            .or_insert_with(|| shader_reads(Some(tokens)))
            .clone()
    }
    fn pass(&mut self, targets: &[Option<Target>; 4], depth: &Option<Target>) -> &mut Pass {
        if self
            .passes
            .last()
            .is_none_or(|p| &p.key.0 != targets || &p.key.1 != depth)
        {
            if let Some(p) = self.passes.last_mut() {
                p.finish();
            }
            self.passes.push(Pass::new(
                self.passes.len(),
                self.totals.commands - 1,
                targets,
                depth,
            ));
        }
        let p = self.passes.last_mut().unwrap();
        p.last_command = self.totals.commands - 1;
        p
    }
    fn push(&mut self, command: Command) {
        let ordinal = self.totals.commands;
        self.totals.commands += 1;
        *self
            .totals
            .operations
            .entry(command.kind().to_owned())
            .or_default() += 1;
        if !matches!(command, Command::Draw(_)) {
            if let Some(p) = self.passes.last_mut() {
                p.break_runs();
                if !matches!(command, Command::Clear(_)) {
                    p.last_command = ordinal;
                }
            }
        }
        match command {
            Command::Draw(d) => {
                let vs_reads = self.reads(d.vertex_shader.as_deref());
                let ps_reads = self.reads(d.pixel_shader.as_deref());
                let changed = self.previous.as_ref().map(|p| changes(p, &d));
                let vs = self
                    .previous
                    .as_ref()
                    .map(|p| delta(&p.vs_constants, &d.vs_constants));
                let ps = self
                    .previous
                    .as_ref()
                    .map(|p| delta(&p.ps_constants, &d.ps_constants));
                let same = self
                    .previous
                    .as_ref()
                    .map(|p| [batch_equal(p, &d, false), batch_equal(p, &d, true)])
                    .unwrap_or([false; 2]);
                let draw = self.totals.draws;
                self.totals.draws += 1;
                let p = self.pass(&d.targets, &d.depth);
                p.totals.draws += 1;
                p.totals.primitives += u64::from(d.primitives);
                if let Some(changed) = changed {
                    p.totals.comparisons += 1;
                    for (n, c) in p.totals.changed.iter_mut().zip(changed) {
                        *n += u64::from(c);
                    }
                }
                if let Some((registers, bytes)) = vs {
                    p.totals.vs_float4_changed += registers;
                    p.totals.vs_bytes_changed += bytes;
                }
                if let Some((registers, bytes)) = ps {
                    p.totals.ps_float4_changed += registers;
                    p.totals.ps_bytes_changed += bytes;
                }
                for (i, equal) in same.into_iter().enumerate() {
                    if p.runs[i] > 0 && !equal {
                        let hist = if i == 0 {
                            &mut p.totals.runs_same_constants
                        } else {
                            &mut p.totals.runs_ignoring_constants
                        };
                        *hist.entry(p.runs[i]).or_default() += 1;
                        p.runs[i] = 0;
                    }
                    p.runs[i] += 1;
                }
                p.shaders.insert((
                    d.vertex_shader.as_ref().map(|s| s.to_vec()),
                    d.pixel_shader.as_ref().map(|s| s.to_vec()),
                ));
                // Exact content keys (no hash collision approximation). Sampler
                // states are included; order is the recorded list order.
                p.textures.insert(format!("{:?}", d.textures));
                p.states.insert(d.render_states.to_vec());
                p.draws.push(DrawStats {
                    command: ordinal,
                    draw,
                    changed,
                    vs_float4_changed: vs.map(|v| v.0),
                    ps_float4_changed: ps.map(|v| v.0),
                    vs_bytes_changed: vs.map(|v| v.1),
                    ps_bytes_changed: ps.map(|v| v.1),
                    vs_reads,
                    ps_reads,
                });
                self.previous = Some(d);
            }
            Command::Clear(c) => {
                let pass = self.pass(&c.targets, &c.depth).ordinal;
                let viewport = [
                    i64::from(c.viewport[0]),
                    i64::from(c.viewport[1]),
                    i64::from(c.viewport[0]) + i64::from(c.viewport[2]),
                    i64::from(c.viewport[1]) + i64::from(c.viewport[3]),
                ];
                let rects = if c.rects.is_empty() {
                    vec![viewport]
                } else {
                    c.rects.iter().map(|r| r.map(i64::from)).collect()
                };
                let mut clear_pixels = 0;
                let targets: Vec<_> = c
                    .targets
                    .iter()
                    .flatten()
                    .filter(|_| c.flags & 1 != 0)
                    .chain(c.depth.iter().filter(|_| c.flags & 6 != 0))
                    .map(|t| {
                        let pixels: u64 = rects
                            .iter()
                            .map(|r| {
                                area(intersect(
                                    intersect(*r, viewport),
                                    intersect(
                                        [0, 0, t.width.into(), t.height.into()],
                                        c.scissor.map(|r| r.map(i64::from)).unwrap_or([
                                            0,
                                            0,
                                            t.width.into(),
                                            t.height.into(),
                                        ]),
                                    ),
                                ))
                            })
                            .sum();
                        clear_pixels += pixels;
                        json!({"target":target_json(t), "rectangle_pixel_visits":pixels})
                    })
                    .collect();
                *self
                    .totals
                    .operation_pixel_visits
                    .entry("clear_written_targets".into())
                    .or_default() += clear_pixels;
                self.events.push(
                    json!({"command":ordinal,"kind":"clear","pass":pass,"flags":c.flags,
                    "rects":c.rects,"viewport":c.viewport,"scissor":c.scissor,"writes":targets}),
                );
            }
            Command::Copy(c) => {
                *self
                    .totals
                    .operation_pixel_visits
                    .entry("copy_source_requested".into())
                    .or_default() += area(c.src_rect.map(i64::from));
                *self
                    .totals
                    .operation_pixel_visits
                    .entry("copy_destination_requested".into())
                    .or_default() += area(c.dst_rect.map(i64::from));
                self.events.push(json!({"command":ordinal,"kind":"copy",
                "src":target_json(&c.src),"dst":target_json(&c.dst),"src_rect":c.src_rect,"dst_rect":c.dst_rect,
                "src_pixels":area(c.src_rect.map(i64::from)),"dst_pixels":area(c.dst_rect.map(i64::from)),"filter":c.filter}));
            }
            Command::Fill(c) => {
                *self
                    .totals
                    .operation_pixel_visits
                    .entry("fill_requested".into())
                    .or_default() += area(c.rect.map(i64::from));
                self.events
                    .push(json!({"command":ordinal,"kind":"fill","dst":target_json(&c.dst),
                "rect":c.rect,"pixels":area(c.rect.map(i64::from))}));
            }
            Command::Present(_) => self.complete = true,
            _ => {}
        }
    }
    fn finish(&mut self) {
        if let Some(p) = self.passes.last_mut() {
            p.finish();
        }
        self.totals.draws = 0; // accumulated from passes below
        for p in &self.passes {
            self.totals.add(&p.totals);
        }
        self.totals.frames = 1;
        self.totals.passes = self.passes.len() as u64;
        self.previous = None;
        self.shader_cache.clear();
    }
}

fn intersect(a: [i64; 4], b: [i64; 4]) -> [i64; 4] {
    [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])]
}
fn area(r: [i64; 4]) -> u64 {
    (r[2] - r[0]).max(0) as u64 * (r[3] - r[1]).max(0) as u64
}

fn print_totals(out: &mut dyn Write, t: &Totals) -> io::Result<()> {
    if !t.operation_pixel_visits.is_empty() {
        writeln!(
            out,
            "  operation pixel visits (not bandwidth): {:?}",
            t.operation_pixel_visits
        )?;
    }
    writeln!(
        out,
        "  {} draws, {} primitives; {} predecessor comparisons",
        t.draws, t.primitives, t.comparisons
    )?;
    for (group, count) in GROUPS.iter().zip(t.changed) {
        writeln!(
            out,
            "    {group:20} {count:6} changed ({:6.2}%)",
            if t.comparisons == 0 {
                0.0
            } else {
                100.0 * count as f64 / t.comparisons as f64
            }
        )?;
    }
    writeln!(
        out,
        "  constants changed: VS {} float4 / {} bytes; PS {} float4 / {} bytes (includes int/bool bytes)",
        t.vs_float4_changed, t.vs_bytes_changed, t.ps_float4_changed, t.ps_bytes_changed
    )?;
    writeln!(
        out,
        "  run length -> runs (including singletons): same constants {:?}; ignoring constants {:?}",
        t.runs_same_constants, t.runs_ignoring_constants
    )
}

fn print_frame(out: &mut dyn Write, f: &Frame) -> io::Result<()> {
    writeln!(
        out,
        "\nFRAME {}{}: {} commands, {} passes; operations {:?}",
        f.frame,
        if f.complete { "" } else { " (partial: no Present)" },
        f.totals.commands,
        f.passes.len(),
        f.totals.operations
    )?;
    for p in &f.passes {
        writeln!(
            out,
            " PASS {} commands {}..={}: {}",
            p.ordinal, p.first_command, p.last_command, p.name
        )?;
        writeln!(
            out,
            "  distinct shader pairs {}, texture/sampler sets {}, render-state lists {}",
            p.distinct_shader_pairs, p.distinct_texture_sets, p.distinct_render_state_lists
        )?;
        print_totals(out, &p.totals)?;
        writeln!(
            out,
            "  draw/cmd   VS delta(reg/B) PS delta(reg/B)  static VS/PS float4 reads (* = relative; ? = unavailable)  changed group indices"
        )?;
        for d in &p.draws {
            let count = |x: Option<u64>| x.map(|x| x.to_string()).unwrap_or_else(|| "?".into());
            let reads = |r: &Reads| {
                format!(
                    "{}{}",
                    r.static_float4
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "?".into()),
                    if r.relative { "*" } else { "" }
                )
            };
            let groups = d
                .changed
                .map(|c| {
                    c.iter()
                        .enumerate()
                        .filter(|(_, v)| **v)
                        .map(|(i, _)| i.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_else(|| "baseline unknown".into());
            writeln!(
                out,
                "  {:5}/{:<5} {:>4}/{:<6} {:>4}/{:<6} {:>5}/{:<5} {}",
                d.draw,
                d.command,
                count(d.vs_float4_changed),
                count(d.vs_bytes_changed),
                count(d.ps_float4_changed),
                count(d.ps_bytes_changed),
                reads(&d.vs_reads),
                reads(&d.ps_reads),
                groups
            )?;
        }
    }
    for event in &f.events {
        writeln!(out, " EVENT {event}")?;
    }
    Ok(())
}

fn analyze<R: Read>(
    mut reader: Reader<R>,
    range: Range<u64>,
    out: &mut dyn Write,
    mut json_out: Option<&mut dyn Write>,
) -> io::Result<Totals> {
    writeln!(
        out,
        "D3D9 frame stats: zero-based frames, half-open selection {}..{}",
        range.start, range.end
    )?;
    writeln!(
        out,
        "Group indices: {:?}. First draw per frame is an unknown baseline; percentages use predecessor comparisons. Reads are static references, not execution counts. Runs are candidates only; every non-draw is a barrier. Reader retains its global blob dictionary.",
        GROUPS
    )?;
    if let Some(w) = json_out.as_deref_mut() {
        write!(
            w,
            "{{\"schema_version\":1,\"groups\":{},\"frames\":[",
            json!(GROUPS)
        )?;
    }
    let mut total = Totals::default();
    let mut frame = Frame::new(0);
    let mut first = true;
    let mut emit = |frame: &mut Frame, total: &mut Totals| -> io::Result<()> {
        frame.finish();
        print_frame(out, frame)?;
        if let Some(w) = json_out.as_deref_mut() {
            if !first {
                write!(w, ",")?;
            }
            serde_json::to_writer(w, frame)?;
        }
        first = false;
        total.add(&frame.totals);
        Ok(())
    };
    while frame.frame < range.end {
        let Some(command) = reader.next() else {
            break;
        };
        let command = command?;
        let present = matches!(command, Command::Present(_));
        if range.contains(&frame.frame) {
            frame.push(command);
        }
        if present {
            if range.contains(&frame.frame) {
                emit(&mut frame, &mut total)?;
            }
            frame = Frame::new(frame.frame + 1);
        }
    }
    if range.contains(&frame.frame) && frame.totals.commands > 0 {
        emit(&mut frame, &mut total)?;
    }
    writeln!(
        out,
        "\nSUMMARY: {} selected frames (including partial), {} passes, {} commands; operations {:?}",
        total.frames, total.passes, total.commands, total.operations
    )?;
    print_totals(out, &total)?;
    if let Some(w) = json_out {
        write!(w, "],\"summary\":")?;
        serde_json::to_writer(&mut *w, &total)?;
        writeln!(w, "}}")?;
    }
    Ok(total)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let input = args
        .next()
        .ok_or("usage: frame_stats <file.ddframe> [--frames A..B] [--json out.json]")?;
    let mut range = 0..u64::MAX;
    let mut json_path = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => {
                let value = args.next().ok_or("--frames requires A..B")?;
                let (a, b) = value.split_once("..").ok_or("--frames requires A..B")?;
                range = a.parse()?..b.parse()?;
                if range.start >= range.end {
                    return Err("--frames requires A < B".into());
                }
            }
            "--json" => json_path = Some(args.next().ok_or("--json requires a path")?),
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    // create_new prevents accidentally truncating the input or an earlier report.
    let reader = Reader::open(input)?;
    let mut json_file = json_path
        .map(|p| {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(p)
                .map(BufWriter::new)
        })
        .transpose()?;
    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    analyze(
        reader,
        range,
        &mut out,
        json_file.as_mut().map(|f| f as &mut dyn Write),
    )?;
    out.flush()?;
    if let Some(w) = json_file.as_mut() {
        w.flush()?;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("frame_stats: {e}");
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use d3d9cmd::{ClearCall, CopyCall, FillCall, Indices, Upload, Vertices};
    use d3d9frame::Writer;

    // Small synthetic stream following the crate's every_command_reads_back_as_written
    // helper. No installed game, graphics backend or GPU is involved.
    fn target(surface: u32) -> Target {
        Target {
            surface,
            format: 21,
            width: 8,
            height: 4,
            texture: None,
            level: 0,
            samples: 0,
            quality: 0,
        }
    }
    fn draw() -> DrawCall {
        DrawCall {
            primitive: 4,
            primitives: 2,
            start: 0,
            base_vertex: 0,
            min_index: 0,
            vertex_count: 6,
            vertices: Vertices::Streams(vec![(0, 10, 0, 12, 1)]),
            indices: Indices::Buffer(11, 101),
            vertex_shader: Some(vec![0xfffe0300, 0x02000001, 0xc00f0000, 0xa0e40002, 0xffff].into()),
            pixel_shader: None,
            declaration: vec![[0; 8]].into(),
            fvf: 0,
            vs_constants: Constants {
                f: vec![[0; 4]; 4],
                ..Default::default()
            }
            .into(),
            ps_constants: Constants::default().into(),
            textures: vec![],
            render_states: vec![(7, 1)].into(),
            viewport: [0, 0, 8, 4, 0, 0x3f800000],
            scissor: [0, 0, 8, 4],
            targets: [Some(target(1)), None, None, None],
            depth: None,
        }
    }
    fn clear(surface: u32) -> Command {
        Command::Clear(ClearCall {
            rects: vec![],
            flags: 1,
            colour: 0,
            z: 1.0,
            stencil: 0,
            targets: [Some(target(surface)), None, None, None],
            depth: None,
            viewport: [0, 0, 8, 4, 0, 0x3f800000],
            scissor: Some([1, 1, 4, 3]),
            srgb: false,
        })
    }
    fn capture(commands: &[Command]) -> Vec<u8> {
        let mut writer = Writer::new(Vec::new()).unwrap();
        for command in commands {
            writer.write(command.as_cmd()).unwrap();
        }
        writer.into_inner()
    }
    fn report(commands: &[Command], range: Range<u64>) -> (Totals, Value, String) {
        let bytes = capture(commands);
        let mut text = vec![];
        let mut json = vec![];
        let totals = analyze(
            Reader::new(&bytes[..]).unwrap(),
            range,
            &mut text,
            Some(&mut json),
        )
        .unwrap();
        (
            totals,
            serde_json::from_slice(&json).unwrap(),
            String::from_utf8(text).unwrap(),
        )
    }
    fn sample() -> Vec<Command> {
        let a = draw();
        let mut b = a.clone();
        b.start = 6;
        let mut c = b.clone();
        std::sync::Arc::make_mut(&mut c.vs_constants).f[2][0] = 0x3f800000;
        let mut d = c.clone();
        d.targets[0] = Some(target(2));
        d.render_states[0].1 = 0;
        vec![
            clear(1),
            Command::Draw(a),
            Command::Draw(b),
            Command::Draw(c.clone()),
            Command::Upload(Upload::Buffer {
                resource: 10,
                bytes: vec![0; 24],
            }),
            Command::Draw(c),
            clear(2),
            Command::Draw(d),
            Command::Copy(CopyCall {
                src: target(2),
                dst: target(1),
                src_rect: [0, 0, 8, 4],
                dst_rect: [0, 0, 4, 2],
                filter: 1,
            }),
            Command::Fill(FillCall {
                dst: target(1),
                rect: [0, 0, 2, 2],
                colour: 0,
            }),
            Command::Present(target(1)),
            Command::Draw(draw()),
            Command::Present(target(1)),
        ]
    }

    #[test]
    fn writer_reader_passes_redundancy_and_batching() {
        if let Ok(path) = std::env::var("DDDA_FRAME_STATS_SAMPLE") {
            std::fs::write(path, capture(&sample())).unwrap();
        }
        let (t, j, text) = report(&sample(), 0..2);
        assert_eq!(
            (t.frames, t.passes, t.draws, t.primitives, t.comparisons),
            (2, 3, 6, 12, 4)
        );
        assert_eq!(t.vs_float4_changed, 1);
        assert_eq!(t.vs_bytes_changed, 16);
        assert_eq!(t.operation_pixel_visits["clear_written_targets"], 12);
        assert_eq!(t.operation_pixel_visits["copy_source_requested"], 32);
        assert_eq!(t.operation_pixel_visits["copy_destination_requested"], 8);
        assert_eq!(t.operation_pixel_visits["fill_requested"], 4);
        assert_eq!(t.changed, [0, 0, 1, 0, 0, 0, 1, 0, 0, 1]);
        assert_eq!(t.runs_same_constants, BTreeMap::from([(1, 4), (2, 1)]));
        assert_eq!(t.runs_ignoring_constants, BTreeMap::from([(1, 3), (3, 1)]));
        let p = &j["frames"][0]["passes"];
        assert_eq!(p[0]["last_command"], 5);
        assert_eq!(p[1]["first_command"], 6);
        assert_eq!(p[0]["distinct_shader_pairs"], 1);
        assert_eq!(p[0]["draws"][0]["vs_bytes_changed"], Value::Null);
        assert_eq!(p[0]["draws"][2]["vs_bytes_changed"], 16);
        assert_eq!(p[0]["draws"][0]["vs_reads"]["static_float4"], 1);
        assert!(text.contains("25.00%"));
        assert_eq!(
            j["frames"][0]["events"][0]["writes"][0]["rectangle_pixel_visits"],
            6
        );
        assert_eq!(j["frames"][0]["events"][2]["src_pixels"], 32);
        assert_eq!(j["frames"][0]["events"][2]["dst_pixels"], 8);
        assert_eq!(j["frames"][0]["events"][3]["pixels"], 4);
    }

    #[test]
    fn selection_empty_frames_and_partial_tail() {
        let (t, j, _) = report(&sample(), 1..2);
        assert_eq!((t.frames, t.draws, t.comparisons), (1, 1, 0));
        assert_eq!(j["frames"][0]["frame"], 1);
        let (t, j, _) = report(&[Command::Present(target(1)), Command::Draw(draw())], 0..4);
        assert_eq!((t.frames, t.draws), (2, 1));
        assert_eq!(j["frames"][1]["complete"], false);
        let (t, j, _) = report(&sample(), 20..21);
        assert_eq!(t.frames, 0);
        assert_eq!(j["frames"], json!([]));
    }

    #[test]
    fn constants_are_bit_exact_and_include_ints_bools() {
        let a = Constants {
            f: vec![[0x7fc00001, 0, 0, 0]],
            ..Default::default()
        };
        let mut b = a.clone();
        assert_eq!(delta(&a, &b), (0, 0));
        b.f[0][0] = 0x7fc00002;
        b.f.push([0; 4]);
        b.i[0][0] = 4;
        b.b[0] = 1;
        assert_eq!(delta(&a, &b), (1, 36));
        b.f[1][0] = 0x80000000;
        assert_eq!(delta(&a, &b), (2, 52));
    }

    #[test]
    fn batching_checks_all_non_range_state_and_barriers() {
        let a = draw();
        let mut b = a.clone();
        b.start = 9;
        b.base_vertex = -4;
        b.vertex_count = 3;
        b.primitives = 1;
        b.min_index = 2;
        assert!(batch_equal(&a, &b, false));
        b.primitive = 5;
        assert!(!batch_equal(&a, &b, true));
        b = a.clone();
        b.scissor[0] = 1;
        assert!(!batch_equal(&a, &b, true));
        b = a.clone();
        b.fvf = 1;
        assert!(!batch_equal(&a, &b, true));
        b = a.clone();
        b.indices = Indices::Buffer(12, 101);
        assert!(!batch_equal(&a, &b, true));
        b = a.clone();
        b.vertices = Vertices::Streams(vec![(0, 10, 12, 12, 1)]);
        assert!(!batch_equal(&a, &b, true));
        let (t, _, _) = report(
            &[
                Command::Draw(a.clone()),
                Command::QueryBegin(7),
                Command::Draw(a),
                Command::Present(target(1)),
            ],
            0..1,
        );
        assert_eq!(t.runs_ignoring_constants, BTreeMap::from([(1, 2)]));
    }

    #[test]
    fn target_identity_includes_mrt_depth_and_revisits() {
        let a = draw();
        let mut b = a.clone();
        b.targets[1] = Some(target(2));
        let mut c = b.clone();
        c.depth = Some(Target {
            format: 75,
            ..target(3)
        });
        let (t, j, _) = report(
            &[
                Command::Draw(a.clone()),
                Command::Draw(b),
                Command::Draw(c),
                Command::Draw(a),
                Command::Present(target(1)),
            ],
            0..1,
        );
        assert_eq!(t.passes, 4);
        assert_eq!(j["frames"][0]["passes"][2]["depth"]["format_name"], "D24S8");
    }

    #[test]
    fn shader_static_reads_matrix_locals_relative_and_invalid() {
        let tokens = [
            0xfffe0300, 0x05000051, 0xa00f0002, 0, 0, 0, 0, // def c2
            0x03000014, 0xc00f0000, 0x90e40000, 0xa0e40001, // m4x4 o0,v0,c1 -> c1..4
            0xffff,
        ];
        assert_eq!(shader_reads(Some(&tokens)).static_float4, Some(3));
        let tokens = [0xfffe0300, 0x03000001, 0xc00f0000, 0xa0e42001, 0xb0000000, 0xffff];
        assert!(shader_reads(Some(&tokens)).relative);
        assert_eq!(shader_reads(Some(&[0])).status, "parse_error");
        assert_eq!(shader_reads(None).status, "fixed_function");
    }

    #[test]
    fn many_frames_stream_json_without_retaining_reports() {
        let mut w = Writer::new(Vec::new()).unwrap();
        let d = draw();
        let t = target(1);
        for _ in 0..1000 {
            w.write(d3d9frame::Cmd::Draw(&d)).unwrap();
            w.write(d3d9frame::Cmd::Present(&t)).unwrap();
        }
        let bytes = w.into_inner();
        let t = analyze(
            Reader::new(&bytes[..]).unwrap(),
            0..1000,
            &mut io::sink(),
            Some(&mut io::sink()),
        )
        .unwrap();
        assert_eq!((t.frames, t.draws, t.comparisons), (1000, 1000, 0));
    }
}
