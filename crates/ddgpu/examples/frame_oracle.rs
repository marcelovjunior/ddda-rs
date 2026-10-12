//! The frame oracle (docs/FRAME_ORACLE.md): replay a capture written with
//! DDDA_CAPTURE into the wgpu backend and the host's D3D9 driver and
//! compare their targets.
//!
//! cargo run -p ddgpu --example frame_oracle -- <capture> [--frames A..B]
//!     [--last N] [--no-bisect] [--tolerance T] [--images DIR]
//!
//! --frames A..B   compare frames A to B-1 (default: every frame); frames
//!                 before A are replayed without comparing
//! --last N        stop after frame N
//! --no-bisect     only compare at each Present (no per-command read-backs)
//! --tolerance T   allow a decoded channel difference of T (default 0:
//!                 equal bits)
//! --images DIR    write each compared target as wgpu, d3d9 and diff PNGs
//!
//! Exit status 0 when everything compared is equal, 1 when something
//! differs, 2 on a usage or file error.
use d3d9frame::{oracle, Reader};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || -> ! {
        eprintln!("usage: frame_oracle <capture> [--frames A..B] [--last N] [--no-bisect] [--tolerance T] [--images DIR]");
        std::process::exit(2)
    };
    let mut path = None;
    let mut opts = oracle::Options::default();
    let mut k = 0;
    while k < args.len() {
        let value = |k: usize| args.get(k + 1).cloned().unwrap_or_else(|| usage());
        match args[k].as_str() {
            "--frames" => {
                let v = value(k);
                let (a, b) = v.split_once("..").unwrap_or_else(|| usage());
                opts.frames = Some(a.parse().unwrap_or_else(|_| usage())..b.parse().unwrap_or_else(|_| usage()));
                k += 1;
            }
            "--last" => {
                opts.last_frame = Some(value(k).parse().unwrap_or_else(|_| usage()));
                k += 1;
            }
            "--no-bisect" => opts.bisect = false,
            "--tolerance" => {
                opts.tolerance = value(k).parse().unwrap_or_else(|_| usage());
                k += 1;
            }
            "--images" => {
                opts.images = Some(value(k).into());
                k += 1;
            }
            a if a.starts_with("--") || path.is_some() => usage(),
            a => path = Some(a.to_string()),
        }
        k += 1;
    }
    let Some(path) = path else { usage() };
    let reader = Reader::open(&path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(2)
    });
    let Some(mut gpu) = ddgpu::WgpuBackend::headless() else {
        eprintln!("no wgpu adapter");
        std::process::exit(2)
    };
    gpu.read_frames = false;
    let Some(mut driver) = d3d9ref::Driver::new() else {
        eprintln!("no D3D9 device (a full-screen application holds the adapter, or there is no D3D9)");
        std::process::exit(2)
    };
    let report = oracle::compare(reader, &mut gpu, |g| std::mem::take(&mut g.errors), &mut driver, |d| std::mem::take(&mut d.errors), &opts)
        .unwrap_or_else(|e| {
            eprintln!("{path}: {e}");
            std::process::exit(2)
        });
    println!("{report}");
    std::process::exit(if report.equal() { 0 } else { 1 })
}
