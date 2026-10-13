//! L83 phase benchmark. module_cost <skip> <count> <cache-dir> [cpu]
//! Distinct vertex AND pixel shaders, sorted package order, from the install.
//! Run bounded batches; no pipelines, draws, or window are created.
mod shader_cache;
use std::{
    borrow::Cow,
    time::{Duration, Instant},
};

fn block_on<F: std::future::Future>(f: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let w = std::task::Waker::from(std::sync::Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&w);
    let mut f = std::pin::pin!(f);
    loop {
        if let std::task::Poll::Ready(x) = f.as_mut().poll(&mut cx) {
            return x;
        }
        std::thread::park();
    }
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let skip: usize = args[0].parse().unwrap();
    let count: usize = args[1].parse().unwrap();
    let dir = std::path::Path::new(&args[2]);
    let cpu = args.get(3).is_some_and(|s| s == "cpu");
    let device = if cpu {
        None
    } else {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        if let Ok(backend) = std::env::var("DDGPU_BENCH_BACKEND") {
            descriptor.backends = match backend.as_str() {
                "vulkan" => wgpu::Backends::VULKAN,
                "dx12" => wgpu::Backends::DX12,
                _ => panic!("DDGPU_BENCH_BACKEND must be vulkan or dx12"),
            };
        }
        eprintln!("creating instance: {:?}", descriptor.backends);
        let instance = wgpu::Instance::new(descriptor);
        eprintln!("requesting adapter");
        let adapter = block_on(instance.request_adapter(&Default::default())).unwrap();
        eprintln!("adapter: {:?}", adapter.get_info());
        let (device, _) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: ddgpu::WgpuBackend::required_features()
                | (adapter.features() & ddgpu::WgpuBackend::optional_features()),
            ..Default::default()
        }))
        .unwrap();
        Some(device)
    };
    let root = std::env::var_os("DDDA_SHADER_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| r"E:\SteamLibrary\steamapps\common\DDDA\nativePC\sa\DX9".into());
    let mut paths: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "arc"))
        .collect();
    paths.sort();
    let mut seen = std::collections::HashSet::new();
    let mut times = [Duration::ZERO; 8];
    let mut n = 0;
    let mut hits = 0;
    let mut stages = [0; 2];
    'packages: for path in paths {
        let (archive, mut file) = mtarc::Archive::open(path).unwrap();
        for entry in archive
            .entries
            .iter()
            .filter(|e| e.type_hash == mtarc::type_hash("rShaderPackage"))
        {
            let bytes = mtarc::read_entry(&mut file, entry).unwrap();
            let tokens = d3d9shader::tokens(&bytes);
            let mut k = 0;
            while k < tokens.len() {
                if matches!(tokens[k], 0xFFFE_0300 | 0xFFFF_0300) {
                    if let Ok(s) = d3d9shader::parse(&tokens[k..]) {
                        let slice = &tokens[k..k + s.tokens];
                        k += s.tokens;
                        if !s.instructions.iter().any(|i| i.opcode != d3d9shader::op::COMMENT)
                            || !seen.insert(slice.to_vec())
                        {
                            continue;
                        }
                        if seen.len() <= skip {
                            continue;
                        }
                        let start = Instant::now();
                        let s = d3d9shader::parse(slice).unwrap();
                        let t = d3d9wgsl::translate(&s).unwrap();
                        times[0] += start.elapsed();
                        let start = Instant::now();
                        let ir = naga::front::wgsl::parse_str(&t.wgsl).unwrap();
                        times[1] += start.elapsed();
                        let start = Instant::now();
                        naga::valid::Validator::new(
                            naga::valid::ValidationFlags::all(),
                            naga::valid::Capabilities::all(),
                        )
                        .validate(&ir)
                        .unwrap();
                        times[2] += start.elapsed();
                        let start = Instant::now();
                        let cached = shader_cache::load(dir, &t.wgsl);
                        times[4] += start.elapsed();
                        let restored = if let Some(module) = cached {
                            hits += 1;
                            module
                        } else {
                            let start = Instant::now();
                            shader_cache::save(dir, &t.wgsl, &ir).unwrap();
                            times[3] += start.elapsed();
                            shader_cache::load(dir, &t.wgsl).expect("cache roundtrip")
                        };
                        let encoded = bincode::serialize(&ir).unwrap();
                        let start = Instant::now();
                        let _: naga::Module = bincode::deserialize(&encoded).unwrap();
                        times[7] += start.elapsed();
                        // Serialize both IRs again: exact structure, spans and float bits.
                        assert_eq!(
                            bincode::serialize(&ir).unwrap(),
                            bincode::serialize(&restored).unwrap()
                        );
                        if let Some(device) = &device {
                            for (index, source) in [
                                (5, wgpu::ShaderSource::Wgsl(Cow::Borrowed(&t.wgsl))),
                                (6, wgpu::ShaderSource::Naga(Cow::Owned(restored))),
                            ] {
                                let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
                                let start = Instant::now();
                                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                                    label: None,
                                    source,
                                });
                                times[index] += start.elapsed();
                                assert!(block_on(scope.pop()).is_none());
                                drop(module);
                            }
                        }
                        n += 1;
                        if n % 1000 == 0 {
                            eprintln!("checked {n} shaders");
                        }
                        stages[usize::from(slice[0] == 0xFFFF_0300)] += 1;
                        if n == count {
                            break 'packages;
                        }
                        continue;
                    }
                }
                k += 1;
            }
        }
    }
    println!(
        "count={n} skip={skip} vertex={} pixel={} cpu={cpu} cache_hits={hits}",
        stages[0], stages[1]
    );
    for (label, time) in [
        "translate",
        "parse",
        "validate",
        "save",
        "load",
        "wgsl_module",
        "naga_module",
        "decode_memory",
    ]
    .into_iter()
    .zip(times)
    {
        println!(
            "{label}: total_ms={:.6} mean_ms={:.6}",
            time.as_secs_f64() * 1e3,
            time.as_secs_f64() * 1e3 / n as f64
        );
    }
}
