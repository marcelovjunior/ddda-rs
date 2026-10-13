//! A captured command stream replayed into two backends (brief L41: the
//! wgpu backend and the host's D3D9 driver), their targets compared frame
//! by frame. Frame k is the commands up to and including the k-th Present
//! (from 0). In a compared frame:
//!
//! - at the Present, the back buffer and every colour target the frame
//!   wrote are read back from both and compared ([`TargetReport`]; with an
//!   image directory, each as two PNGs and a difference image);
//! - with `bisect`, the targets each command writes are read back after it,
//!   and the first command that leaves a target differing which was equal
//!   before it is reported ([`FirstDifference`]); a target that already
//!   differs when the frame first writes it is listed in `already`.
//!
//! Multisampled targets and depth-stencil surfaces cannot be read back
//! (D3D9's GetRenderTargetData refuses them): they are compared through
//! what is resolved or drawn from them.
use crate::image::{self, Difference};
use crate::Command;
use d3d9cmd::{GraphicsBackend, Target};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

pub struct Options {
    /// the frames compared; None: every frame
    pub frames: Option<std::ops::Range<u32>>,
    /// stop after this frame
    pub last_frame: Option<u32>,
    /// read back what each command writes, to name the first differing one
    pub bisect: bool,
    /// a decoded channel difference allowed (unorm levels or float value);
    /// 0: the bits must be equal
    pub tolerance: f32,
    /// where images go; None: no images
    pub images: Option<PathBuf>,
    /// the two backends' names in reports and image names
    pub names: [String; 2],
}

impl Default for Options {
    fn default() -> Self {
        Options { frames: None, last_frame: None, bisect: true, tolerance: 0.0, images: None, names: ["wgpu".into(), "d3d9".into()] }
    }
}

/// A target compared at the end of a frame.
#[derive(Clone, Debug)]
pub struct TargetReport {
    pub frame: u32,
    pub target: Target,
    pub back_buffer: bool,
    pub difference: Difference,
    pub images: Vec<PathBuf>,
}

/// The first command that made a target differ.
#[derive(Clone, Debug)]
pub struct FirstDifference {
    pub frame: u32,
    /// the command's position in its frame and in the stream
    pub command: usize,
    pub index: u64,
    pub kind: &'static str,
    /// what the command was (shaders, primitives, targets, textures)
    pub summary: String,
    pub target: Target,
    pub difference: Difference,
    pub images: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Presents replayed
    pub frames: u32,
    pub commands: u64,
    pub compared: Vec<u32>,
    pub targets: Vec<TargetReport>,
    pub first: Option<FirstDifference>,
    /// (frame, target) already differing when the frame first wrote it
    pub already: Vec<(u32, Target)>,
    /// each backend's errors, with the frame and command they came after
    pub errors: [Vec<String>; 2],
    /// targets that could not be read back from a backend
    pub unreadable: Vec<String>,
}

impl Report {
    /// Every compared target equal, no first difference, no errors.
    pub fn equal(&self) -> bool {
        self.first.is_none() && self.already.is_empty() && self.targets.iter().all(|t| t.difference.differing == 0)
            && self.errors.iter().all(|e| e.is_empty()) && self.unreadable.is_empty()
    }
}

fn readable(t: &Target) -> bool {
    t.samples == 0 && image::bytes_per_pixel(t.format).is_some()
}

/// A one-line description of a command for reports.
pub fn describe(c: &Command) -> String {
    let hash = |t: &Option<std::sync::Arc<[u32]>>| match t {
        Some(t) => {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            t.hash(&mut h);
            format!("{:016x}", h.finish())
        }
        None => "none".into(),
    };
    let surf = |t: &Option<Target>| t.as_ref().map_or("-".to_string(), |t| format!("{:#x}:{}", t.surface, t.format));
    match c {
        Command::Draw(d) => format!(
            "primitive type {} x{}, vs {} ps {}, targets [{}] depth {}, textures [{}]",
            d.primitive, d.primitives, hash(&d.vertex_shader), hash(&d.pixel_shader),
            d.targets.iter().map(surf).collect::<Vec<_>>().join(" "), surf(&d.depth),
            d.textures.iter().map(|t| format!("s{}={:#x}:{}", t.sampler, t.resource, t.format)).collect::<Vec<_>>().join(" ")
        ),
        Command::Clear(c) => format!("flags {:#x} colour {:#010x} z {} stencil {}, {} rects, targets [{}]",
                                     c.flags, c.colour, c.z, c.stencil, c.rects.len(), c.targets.iter().map(surf).collect::<Vec<_>>().join(" ")),
        Command::Copy(c) => format!("{:#x} {:?} -> {:#x} {:?} filter {}", c.src.surface, c.src_rect, c.dst.surface, c.dst_rect, c.filter),
        Command::Fill(f) => format!("{:#x} {:?} colour {:#010x}", f.dst.surface, f.rect, f.colour),
        other => other.kind().to_string(),
    }
}

struct Run<'o, 'x, GA, GB> {
    a: &'x mut GA,
    b: &'x mut GB,
    opts: &'o Options,
    report: Report,
}

impl<GA: GraphicsBackend, GB: GraphicsBackend> Run<'_, '_, GA, GB> {
    /// A target read back from both backends and compared.
    fn compare(&mut self, t: &Target) -> Option<(Difference, Vec<u8>, Vec<u8>)> {
        let (ra, rb) = (self.a.read_back(t), self.b.read_back(t));
        match (ra, rb) {
            (Some(ra), Some(rb)) => Some((image::compare(t.format, t.width, &ra, &rb, self.opts.tolerance), ra, rb)),
            (ra, rb) => {
                let which: Vec<&str> = [(ra.is_none(), &self.opts.names[0]), (rb.is_none(), &self.opts.names[1])].iter().filter(|x| x.0).map(|x| x.1.as_str()).collect();
                let msg = format!("target {:#x} (format {}) not read back from {}", t.surface, t.format, which.join(" and "));
                if !self.report.unreadable.contains(&msg) {
                    self.report.unreadable.push(msg);
                }
                None
            }
        }
    }

    /// The two read-backs and their difference as PNGs named `stem`_*.
    fn images(&self, stem: &str, t: &Target, ra: &[u8], rb: &[u8]) -> Vec<PathBuf> {
        let Some(dir) = &self.opts.images else { return vec![] };
        let mut out = Vec::new();
        for (name, rgba) in [
            (self.opts.names[0].as_str(), image::to_rgba8(t.format, ra)),
            (self.opts.names[1].as_str(), image::to_rgba8(t.format, rb)),
            ("diff", image::diff_rgba8(t.format, ra, rb)),
        ] {
            let path = dir.join(format!("{stem}_{name}.png"));
            if rgba.len() == (4 * t.width * t.height) as usize && image::write_png(&path, t.width, t.height, &rgba).is_ok() {
                out.push(path);
            }
        }
        out
    }
}

/// Replay `commands` into backends `a` and `b` (each given with a function
/// that drains its errors) and compare them as `opts` says.
pub fn compare<GA: GraphicsBackend, GB: GraphicsBackend>(
    commands: impl IntoIterator<Item = io::Result<Command>>,
    a: &mut GA, mut errors_a: impl FnMut(&mut GA) -> Vec<String>,
    b: &mut GB, mut errors_b: impl FnMut(&mut GB) -> Vec<String>,
    opts: &Options,
) -> io::Result<Report> {
    if let Some(dir) = &opts.images {
        std::fs::create_dir_all(dir)?;
    }
    let mut run = Run { a, b, opts, report: Report::default() };
    let mut frame = 0u32;
    let mut in_frame = 0usize;
    // the frame's written colour targets, in first-write order
    let mut written: Vec<Target> = Vec::new();
    // whether each target differs now (known in bisected frames)
    let mut differs: HashMap<u32, bool> = HashMap::new();
    for c in commands {
        let c = c?;
        let compared = opts.frames.as_ref().is_none_or(|r| r.contains(&frame));
        let bisect = compared && opts.bisect && run.report.first.is_none();
        let writes: Vec<Target> = c.written().into_iter().filter(|t| readable(t)).cloned().collect();
        if bisect {
            for t in &writes {
                if !differs.contains_key(&t.surface) {
                    let d = run.compare(t).map(|(d, _, _)| d.differing > 0);
                    if d == Some(true) {
                        run.report.already.push((frame, t.clone()));
                    }
                    differs.insert(t.surface, d.unwrap_or(false));
                }
            }
        }
        c.apply(run.a);
        c.apply(run.b);
        for (side, errors) in [errors_a(run.a), errors_b(run.b)].into_iter().enumerate() {
            for e in errors {
                if run.report.errors[side].len() < 200 {
                    run.report.errors[side].push(format!("frame {frame} command {in_frame} ({}): {e}", c.kind()));
                }
            }
        }
        if compared {
            for t in &writes {
                if !written.iter().any(|w| w.surface == t.surface) {
                    written.push(t.clone());
                }
            }
            if bisect {
                for t in &writes {
                    let Some((d, ra, rb)) = run.compare(t) else { continue };
                    let before = differs.insert(t.surface, d.differing > 0).unwrap_or(false);
                    if d.differing > 0 && !before && run.report.first.is_none() {
                        let images = run.images(&format!("first_f{frame:04}_c{in_frame:05}_{:08x}", t.surface), t, &ra, &rb);
                        run.report.first = Some(FirstDifference {
                            frame, command: in_frame, index: run.report.commands, kind: c.kind(), summary: describe(&c),
                            target: t.clone(), difference: d, images,
                        });
                    }
                }
            }
        }
        run.report.commands += 1;
        if let Command::Present(bb) = &c {
            if compared {
                run.report.compared.push(frame);
                let mut targets: Vec<(Target, bool)> = written.iter().filter(|t| t.surface != bb.surface).map(|t| (t.clone(), false)).collect();
                if readable(bb) {
                    targets.push((bb.clone(), true));
                }
                for (t, back_buffer) in targets {
                    let Some((d, ra, rb)) = run.compare(&t) else { continue };
                    let images = run.images(&format!("f{frame:04}_{:08x}", t.surface), &t, &ra, &rb);
                    run.report.targets.push(TargetReport { frame, target: t, back_buffer, difference: d, images });
                }
            }
            frame += 1;
            in_frame = 0;
            written.clear();
            differs.clear();
            run.report.frames = frame;
            if opts.last_frame.is_some_and(|l| frame > l) {
                break;
            }
        } else {
            in_frame += 1;
        }
    }
    Ok(run.report)
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "{} commands, {} frames, compared frames {:?}", self.commands, self.frames, self.compared)?;
        let differing: Vec<&TargetReport> = self.targets.iter().filter(|t| t.difference.differing > 0).collect();
        writeln!(f, "targets compared: {}, differing: {}", self.targets.len(), differing.len())?;
        for t in &self.targets {
            let d = &t.difference;
            write!(f, "  frame {} {} {:#x} (format {}, {}x{}): ", t.frame, if t.back_buffer { "back buffer" } else { "target" },
                   t.target.surface, t.target.format, t.target.width, t.target.height)?;
            if d.differing == 0 {
                writeln!(f, "equal")?;
            } else {
                write!(f, "{} of {} pixels differ, max {}", d.differing, d.pixels, d.max)?;
                if let Some((x, y, va, vb)) = d.first {
                    write!(f, ", first at ({x}, {y}): {va:?} vs {vb:?}")?;
                }
                writeln!(f)?;
            }
        }
        match &self.first {
            Some(x) => {
                writeln!(f, "first difference: frame {} command {} (stream {}), {}: {}", x.frame, x.command, x.index, x.kind, x.summary)?;
                write!(f, "  target {:#x} (format {}): {} of {} pixels differ, max {}", x.target.surface, x.target.format,
                       x.difference.differing, x.difference.pixels, x.difference.max)?;
                if let Some((px, py, va, vb)) = x.difference.first {
                    write!(f, ", first at ({px}, {py}): {va:?} vs {vb:?}")?;
                }
                writeln!(f)?;
            }
            None => writeln!(f, "first difference: none")?,
        }
        for (frame, t) in &self.already {
            writeln!(f, "already differing when frame {frame} first wrote it: {:#x} (format {})", t.surface, t.format)?;
        }
        for (side, errors) in self.errors.iter().enumerate() {
            for e in errors.iter().take(20) {
                writeln!(f, "error (backend {}): {e}", side)?;
            }
            if errors.len() > 20 {
                writeln!(f, "... {} more errors (backend {side})", errors.len() - 20)?;
            }
        }
        for u in &self.unreadable {
            writeln!(f, "{u}")?;
        }
        Ok(())
    }
}
