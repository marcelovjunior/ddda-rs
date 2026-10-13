//! CPU-only linkage validation: no game execution and no GPU device.
use d3d9shader::Shader;
use d3d9wgsl::{Interface, Options};
use std::collections::{BTreeMap, BTreeSet, HashSet};

fn locations(vs: &Interface, ps: &Interface) -> Vec<Option<u32>> {
    // Same semantic/index linkage as ddgpu::WgpuBackend::try_draw.
    ps.inputs
        .iter()
        .map(|&(usage, index, _)| {
            vs.outputs_vs
                .iter()
                .position(|o| o.0 == usage && o.1 == index)
                .map(|n| n as u32)
        })
        .collect()
}

fn validate(wgsl: &str, label: &str) {
    let module =
        naga::front::wgsl::parse_str(wgsl).unwrap_or_else(|e| panic!("{label}: {}", e.emit_to_string(wgsl)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("{label}: {}", e.emit_to_string(wgsl)));
}

fn validate_pixel(shader: &Shader, mapping: Vec<Option<u32>>, label: &str) {
    let t = d3d9wgsl::translate_with(
        shader,
        Options {
            ps_input_locations: Some(mapping),
            alpha_test: true,
            lod_bias: true,
            depth_bias: true,
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("{label}: {e}"));
    validate(&t.wgsl, label);
}

/// Exercise every distinct linkage map for v0/COLOR0 pixel shaders against
/// installed VS output signatures lacking COLOR0. This is a superset of
/// possible runtime pairings, NOT evidence that the game used each pair.
/// DDDA_ALL_SHADERS=1 covers all packages; otherwise Title and root.
#[test]
fn installed_color0_pairings_validate() {
    let packages = if std::env::var_os("DDDA_ALL_SHADERS").is_some() {
        super::all_packages()
    } else {
        vec!["Title".into(), "root".into()]
    };
    let mut signatures = BTreeMap::new();
    let mut seen_vs = HashSet::new();
    for name in &packages {
        for tokens in super::vertex_shaders(name) {
            if !seen_vs.insert(tokens.clone()) {
                continue;
            }
            let shader = d3d9shader::parse(&tokens).unwrap();
            let t = d3d9wgsl::translate_with(
                &shader,
                Options {
                    half_pixel: true,
                    ..Default::default()
                },
            )
            .unwrap();
            let key: Vec<_> = t.interface.outputs_vs.iter().map(|o| (o.0, o.1)).collect();
            if key.contains(&(10, 0)) || signatures.contains_key(&key) {
                continue;
            }
            let label = format!("{name}:VS:{}", super::tag(&tokens));
            validate(&t.wgsl, &label);
            signatures.insert(key, (label, t.interface));
        }
    }
    let mut seen_ps = HashSet::new();
    let (mut shaders, mut maps) = (0, 0);
    for name in &packages {
        for tokens in super::pixel_shaders(name) {
            if !seen_ps.insert(tokens.clone()) {
                continue;
            }
            let shader = d3d9shader::parse(&tokens).unwrap();
            let iface = d3d9wgsl::translate(&shader).unwrap().interface;
            if !iface.inputs.contains(&(10, 0, 0)) {
                continue;
            }
            shaders += 1;
            let mut seen_maps = BTreeSet::new();
            for (vs_label, vs) in signatures.values() {
                let mapping = locations(vs, &iface);
                if !seen_maps.insert(mapping.clone()) {
                    continue;
                }
                let label = format!("{vs_label} + {name}:PS:{} {mapping:?}", super::tag(&tokens));
                validate_pixel(&shader, mapping, &label);
                maps += 1;
            }
        }
    }
    eprintln!("{shaders} v0/COLOR0 pixel shaders, {} VS output signatures without COLOR0, {maps} distinct shader/linkage maps validated", signatures.len());
    if !seen_ps.is_empty() {
        assert!(shaders > 0 && maps > 0, "installed pairing coverage was empty");
    }
}

/// Validate the exact VS/PS pairs recorded by the game. Supply a private
/// DDDA_SHADER_PAIR_CAPTURE=.ddframe path; nothing is rendered or replayed.
/// Unlike the signature sweep above, these pairings were actually submitted.
#[test]
fn captured_shader_pairings_validate() {
    let Some(path) = std::env::var_os("DDDA_SHADER_PAIR_CAPTURE") else {
        return;
    };
    let reader = d3d9frame::Reader::open(&path).expect("open shader-pair capture");
    let mut seen = HashSet::new();
    let (mut draws, mut missing, mut color0, mut frame) = (0, 0, 0, 0);
    for command in reader {
        match command.expect("read shader-pair capture") {
            d3d9frame::Command::Present(_) => frame += 1,
            d3d9frame::Command::Draw(draw) => {
                draws += 1;
                let (Some(vs), Some(ps)) = (draw.vertex_shader, draw.pixel_shader) else {
                    continue;
                };
                if !seen.insert((vs.clone(), ps.clone())) {
                    continue;
                }
                let label = format!(
                    "frame {frame} draw {draws}: VS:{} PS:{}",
                    super::tag(&vs),
                    super::tag(&ps)
                );
                let vertex = d3d9wgsl::translate_with(
                    &d3d9shader::parse(&vs).expect("captured VS"),
                    Options {
                        half_pixel: true,
                        ..Default::default()
                    },
                )
                .unwrap();
                validate(&vertex.wgsl, &label);
                let pixel = d3d9shader::parse(&ps).expect("captured PS");
                let iface = d3d9wgsl::translate(&pixel).unwrap().interface;
                let mapping = locations(&vertex.interface, &iface);
                if mapping.contains(&None) {
                    missing += 1;
                    if iface
                        .inputs
                        .iter()
                        .zip(&mapping)
                        .any(|(&(u, i, r), l)| (u, i, r) == (10, 0, 0) && l.is_none())
                    {
                        color0 += 1;
                    }
                    eprintln!("unlinked {label}: {mapping:?}");
                }
                validate_pixel(&pixel, mapping, &label);
            }
            _ => {}
        }
    }
    eprintln!("{draws} recorded draws, {} unique VS/PS pairs validated; {missing} with missing inputs, {color0} with missing v0/COLOR0", seen.len());
    assert!(!seen.is_empty(), "capture contains no programmable shader pairs");
}
