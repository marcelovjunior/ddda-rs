//! The oracle runs shaders on the host's D3D9 driver. Skips without D3D9.
use d3d9ref::{Oracle, PixelRun, Texture, TextureKind};

#[test]
fn a_hand_assembled_pixel_shader_computes_input_plus_constant() {
    let Some(mut o) = Oracle::new() else {
        eprintln!("skipped: no D3D9 device");
        return;
    };
    // ps_3_0
    // dcl_texcoord v0
    // add oC0, v0, c3
    let ps = [
        0xFFFF_0300,
        0x0200_001F, 0x8000_0005, 0x900F_0000,
        0x0300_0002, 0x800F_0800, 0x90E4_0000, 0xA0E4_0003,
        0x0000_FFFF,
    ];
    let mut run = PixelRun { varyings: vec![[1.0, 2.0, 3.0, 4.0]], ..PixelRun::default() };
    run.constants = vec![[0.0; 4]; 224];
    run.constants[3] = [0.5, -0.25, 10.0, 0.0];
    let out = o.run_pixel(&ps, &run).expect("runs");
    assert_eq!(out, vec![[1.5, 1.75, 13.0, 4.0]]);
}

/// xorshift32: deterministic pseudo-random inputs.
pub struct Rng(pub u32);
impl Rng {
    pub fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    /// A float in [-2, 2).
    pub fn float(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32 * 4.0 - 2.0
    }
    pub fn vec4(&mut self) -> [f32; 4] {
        [self.float(), self.float(), self.float(), self.float()]
    }
}

fn title_pixel_shaders() -> Vec<Vec<u32>> {
    let path = std::path::Path::new(r"E:\SteamLibrary\steamapps\common\DDDA\nativePC\sa\DX9\Title.arc");
    let Ok((archive, mut file)) = mtarc::Archive::open(path) else { return vec![] };
    let wanted = mtarc::type_hash("rShaderPackage");
    let Some(entry) = archive.entries.iter().find(|e| e.type_hash == wanted) else { return vec![] };
    let tokens = d3d9shader::tokens(&mtarc::read_entry(&mut file, entry).unwrap());
    let mut out = Vec::new();
    let mut k = 0;
    while k < tokens.len() {
        if tokens[k] == 0xFFFF_0300 {
            if let Ok(s) = d3d9shader::parse(&tokens[k..]) {
                if s.instructions.iter().any(|i| i.opcode != d3d9shader::op::COMMENT) {
                    out.push(tokens[k..k + s.tokens].to_vec());
                    k += s.tokens;
                    continue;
                }
            }
        }
        k += 1;
    }
    out
}

#[test]
fn every_title_pixel_shader_runs_on_the_driver() {
    let Some(mut o) = Oracle::new() else {
        eprintln!("skipped: no D3D9 device");
        return;
    };
    let shaders = title_pixel_shaders();
    if shaders.is_empty() {
        eprintln!("skipped: game install not found");
        return;
    }
    let mut rng = Rng(0x1234_5678);
    for ps in &shaders {
        let s = d3d9shader::parse(ps).unwrap();
        let mut run = PixelRun {
            constants: (0..224).map(|_| rng.vec4()).collect(),
            varyings: d3d9ref::pixel_inputs(&s).iter().map(|_| rng.vec4()).collect(),
            ..PixelRun::default()
        };
        for (index, kind) in d3d9ref::samplers(&s) {
            let size = 4;
            let count = match kind {
                TextureKind::Flat => 16,
                TextureKind::Cube => 6 * 16,
                TextureKind::Volume => 64,
            };
            run.textures.resize(index as usize + 1, None);
            run.textures[index as usize] = Some(Texture { kind, size, texels: (0..count).map(|_| rng.vec4()).collect() });
        }
        let out = o.run_pixel(ps, &run).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(out.len(), d3d9ref::colour_outputs(&s).len());
    }
    eprintln!("{} pixel shaders ran", shaders.len());
}
