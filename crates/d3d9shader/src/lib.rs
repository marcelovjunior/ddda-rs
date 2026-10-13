//! Direct3D 9 shader bytecode (shader models 2 and 3): the token stream the
//! game hands to IDirect3DDevice9::CreateVertexShader / CreatePixelShader
//! (docs/RUNTIME.md, "Graphics"). The game's shaders are vs_3_0 and ps_3_0
//! objects in its shader packages (`sc\dx9\<stage>`, rShaderPackage).
//!
//! [`parse`] decodes the tokens into instructions and parameters following
//! the documented format (Microsoft Learn: "Shader Code Format",
//! d3d9types.h); [`disasm`] prints them the way D3DDisassemble does, which is
//! how the parser is checked: tests/disasm.rs compares every shader of the
//! installed game with Microsoft's disassembler. The translator to the
//! runtime's GPU backend builds on this.

pub mod ctab;
pub mod disasm;
pub mod flow;

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Pixel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shader {
    pub stage: Stage,
    pub major: u8,
    pub minor: u8,
    /// Instructions in program order (comments are kept as instructions
    /// with opcode [`op::COMMENT`] so the token stream stays complete).
    pub instructions: Vec<Instruction>,
    /// Number of tokens, version and END included.
    pub tokens: usize,
}

impl Shader {
    /// Exclusive end of the float constant registers read by this shader.
    /// Relative indexing can reach the entire stage register file. Matrix
    /// instructions implicitly read consecutive rows from their second source.
    /// DEF registers remain included (conservative, including relative reads).
    pub fn float_constant_count(&self) -> usize {
        let limit = if self.stage == Stage::Vertex { 256 } else { 224 };
        let mut end = 0;
        for ins in &self.instructions {
            for (source, p) in ins.src.iter().enumerate() {
                if matches!(p.kind, reg::CONST2 | reg::CONST3 | reg::CONST4) {
                    return limit;
                }
                if p.kind != reg::CONST { continue; }
                if p.relative.is_some() { return limit; }
                let rows = if source == 1 {
                    match ins.opcode {
                        op::M4X4 | op::M3X4 => 4,
                        op::M4X3 | op::M3X3 => 3,
                        op::M3X2 => 2,
                        _ => 1,
                    }
                } else { 1 };
                end = end.max((p.index as usize).saturating_add(rows));
            }
        }
        end.min(limit)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instruction {
    pub opcode: u16,
    /// Opcode-specific control bits (16..23): comparison, texld variant.
    pub control: u8,
    pub coissue: bool,
    pub predicated: bool,
    /// The destination, for instructions that have one.
    pub dst: Option<Param>,
    pub src: Vec<Param>,
    /// The predicate register, for predicated instructions.
    pub predicate: Option<Param>,
    /// dcl: the usage token (usage, index, or sampler texture type).
    pub decl: Option<u32>,
    /// def/defi/defb constants (raw tokens); comment payloads.
    pub data: Vec<u32>,
}

/// A register parameter (destination or source) with its modifiers.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub kind: u8,
    pub index: u32,
    /// Destination write mask (bits xyzw) or source swizzle (2 bits per
    /// component, x in bits 0-1).
    pub mask_or_swizzle: u8,
    /// Destination result modifier (saturate 1, partial precision 2,
    /// centroid 4) and shift; source modifier (0..13).
    pub modifier: u8,
    pub shift: u8,
    /// Relative addressing: the address register parameter.
    pub relative: Option<Box<Param>>,
}

/// Opcodes (D3DSHADER_INSTRUCTION_OPCODE_TYPE).
pub mod op {
    pub const NOP: u16 = 0;
    pub const MOV: u16 = 1;
    pub const ADD: u16 = 2;
    pub const SUB: u16 = 3;
    pub const MAD: u16 = 4;
    pub const MUL: u16 = 5;
    pub const RCP: u16 = 6;
    pub const RSQ: u16 = 7;
    pub const DP3: u16 = 8;
    pub const DP4: u16 = 9;
    pub const MIN: u16 = 10;
    pub const MAX: u16 = 11;
    pub const SLT: u16 = 12;
    pub const SGE: u16 = 13;
    pub const EXP: u16 = 14;
    pub const LOG: u16 = 15;
    pub const LIT: u16 = 16;
    pub const DST: u16 = 17;
    pub const LRP: u16 = 18;
    pub const FRC: u16 = 19;
    pub const M4X4: u16 = 20;
    pub const M4X3: u16 = 21;
    pub const M3X4: u16 = 22;
    pub const M3X3: u16 = 23;
    pub const M3X2: u16 = 24;
    pub const CALL: u16 = 25;
    pub const CALLNZ: u16 = 26;
    pub const LOOP: u16 = 27;
    pub const RET: u16 = 28;
    pub const ENDLOOP: u16 = 29;
    pub const LABEL: u16 = 30;
    pub const DCL: u16 = 31;
    pub const POW: u16 = 32;
    pub const CRS: u16 = 33;
    pub const SGN: u16 = 34;
    pub const ABS: u16 = 35;
    pub const NRM: u16 = 36;
    pub const SINCOS: u16 = 37;
    pub const REP: u16 = 38;
    pub const ENDREP: u16 = 39;
    pub const IF: u16 = 40;
    pub const IFC: u16 = 41;
    pub const ELSE: u16 = 42;
    pub const ENDIF: u16 = 43;
    pub const BREAK: u16 = 44;
    pub const BREAKC: u16 = 45;
    pub const MOVA: u16 = 46;
    pub const DEFB: u16 = 47;
    pub const DEFI: u16 = 48;
    pub const TEXCOORD: u16 = 64;
    pub const TEXKILL: u16 = 65;
    pub const TEX: u16 = 66;
    pub const EXPP: u16 = 78;
    pub const LOGP: u16 = 79;
    pub const CND: u16 = 80;
    pub const DEF: u16 = 81;
    pub const CMP: u16 = 88;
    pub const BEM: u16 = 89;
    pub const DP2ADD: u16 = 90;
    pub const DSX: u16 = 91;
    pub const DSY: u16 = 92;
    pub const TEXLDD: u16 = 93;
    pub const SETP: u16 = 94;
    pub const TEXLDL: u16 = 95;
    pub const BREAKP: u16 = 96;
    pub const PHASE: u16 = 0xFFFD;
    pub const COMMENT: u16 = 0xFFFE;
    pub const END: u16 = 0xFFFF;
}

/// Register types (D3DSHADER_PARAM_REGISTER_TYPE).
pub mod reg {
    pub const TEMP: u8 = 0;
    pub const INPUT: u8 = 1;
    pub const CONST: u8 = 2;
    /// a0 in vertex shaders, t# in pixel shaders before 3.0.
    pub const ADDR: u8 = 3;
    pub const RASTOUT: u8 = 4;
    pub const ATTROUT: u8 = 5;
    /// o# in vs_3_0, oT# before.
    pub const OUTPUT: u8 = 6;
    pub const CONSTINT: u8 = 7;
    pub const COLOROUT: u8 = 8;
    pub const DEPTHOUT: u8 = 9;
    pub const SAMPLER: u8 = 10;
    pub const CONST2: u8 = 11;
    pub const CONST3: u8 = 12;
    pub const CONST4: u8 = 13;
    pub const CONSTBOOL: u8 = 14;
    pub const LOOP: u8 = 15;
    pub const TEMPFLOAT16: u8 = 16;
    pub const MISCTYPE: u8 = 17;
    pub const LABEL: u8 = 18;
    pub const PREDICATE: u8 = 19;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The stream is shorter than its tokens say.
    Truncated { at: usize },
    /// Not a vs_/ps_ version token.
    BadVersion(u32),
    /// Only shader models 2 and 3 (instruction lengths in the opcode token).
    Unsupported { major: u8, minor: u8 },
    /// An opcode this decoder does not know.
    UnknownOpcode { at: usize, opcode: u16 },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { at } => write!(f, "token stream ends inside the instruction at token {at}"),
            Error::BadVersion(t) => write!(f, "not a shader version token: {t:#010x}"),
            Error::Unsupported { major, minor } => write!(f, "shader model {major}.{minor} is not supported"),
            Error::UnknownOpcode { at, opcode } => write!(f, "unknown opcode {opcode} at token {at}"),
        }
    }
}

impl std::error::Error for Error {}

/// The tokens of a byte buffer (little-endian 32-bit words; a trailing
/// partial word is ignored).
pub fn tokens(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn param(t: u32) -> Param {
    Param {
        kind: (((t >> 28) & 7) | ((t >> 8) & 0x18)) as u8,
        index: t & 0x7FF,
        mask_or_swizzle: (t >> 16) as u8,
        modifier: ((t >> 20) & 0xF) as u8,
        shift: ((t >> 24) & 0xF) as u8,
        relative: None,
    }
}

/// Source parameters keep their modifier in bits 24..27 and the swizzle in
/// 16..23; destination parameters the write mask in 16..19, the result
/// modifier in 20..23 and the shift in 24..27.
fn src_param(t: u32) -> Param {
    let mut p = param(t);
    p.mask_or_swizzle = (t >> 16) as u8;
    p.modifier = ((t >> 24) & 0xF) as u8;
    p.shift = 0;
    p
}

fn dst_param(t: u32) -> Param {
    let mut p = param(t);
    p.mask_or_swizzle = ((t >> 16) & 0xF) as u8;
    p
}

const RELATIVE: u32 = 1 << 13;

/// Decode a shader starting at `tokens[0]` (its version token) up to and
/// including END. Trailing tokens after END are not read.
pub fn parse(tokens: &[u32]) -> Result<Shader, Error> {
    let version = *tokens.first().ok_or(Error::Truncated { at: 0 })?;
    let stage = match version >> 16 {
        0xFFFE => Stage::Vertex,
        0xFFFF => Stage::Pixel,
        _ => return Err(Error::BadVersion(version)),
    };
    let (major, minor) = ((version >> 8) as u8, version as u8);
    if major < 2 {
        return Err(Error::Unsupported { major, minor });
    }
    let mut at = 1;
    let mut instructions = Vec::new();
    loop {
        let t = *tokens.get(at).ok_or(Error::Truncated { at })?;
        let opcode = t as u16;
        if opcode == op::END {
            at += 1;
            break;
        }
        if opcode == op::COMMENT {
            let n = ((t >> 16) & 0x7FFF) as usize;
            let data = tokens.get(at + 1..at + 1 + n).ok_or(Error::Truncated { at })?.to_vec();
            instructions.push(Instruction { opcode, control: 0, coissue: false, predicated: false, dst: None, src: vec![], predicate: None, decl: None, data });
            at += 1 + n;
            continue;
        }
        let len = ((t >> 24) & 0xF) as usize;
        let body = tokens.get(at + 1..at + 1 + len).ok_or(Error::Truncated { at })?;
        let mut ins = Instruction {
            opcode,
            control: (t >> 16) as u8,
            coissue: t & (1 << 30) != 0,
            predicated: t & (1 << 28) != 0,
            dst: None,
            src: vec![],
            predicate: None,
            decl: None,
            data: vec![],
        };
        let (has_dst, _) = shape(opcode).ok_or(Error::UnknownOpcode { at, opcode })?;
        let mut k = 0;
        match opcode {
            op::DCL => {
                ins.decl = Some(body[0]);
                ins.dst = Some(dst_param(body[1]));
            }
            op::DEF | op::DEFI | op::DEFB => {
                ins.dst = Some(dst_param(body[0]));
                ins.data = body[1..].to_vec();
            }
            _ => {
                if has_dst {
                    let mut d = dst_param(body[k]);
                    k += 1;
                    if body[k - 1] & RELATIVE != 0 && major >= 3 {
                        d.relative = Some(Box::new(src_param(body[k])));
                        k += 1;
                    }
                    ins.dst = Some(d);
                }
                if ins.predicated {
                    // the predicate source follows the destination
                    ins.predicate = Some(src_param(body[k]));
                    k += 1;
                }
                while k < body.len() {
                    let mut s = src_param(body[k]);
                    k += 1;
                    if body[k - 1] & RELATIVE != 0 {
                        if major >= 3 || stage == Stage::Pixel {
                            s.relative = Some(Box::new(src_param(*body.get(k).ok_or(Error::Truncated { at })?)));
                            k += 1;
                        } else {
                            // vs_2_x: relative to a0.x, no extra token
                            s.relative = Some(Box::new(Param { kind: reg::ADDR, index: 0, mask_or_swizzle: 0, modifier: 0, shift: 0, relative: None }));
                        }
                    }
                    ins.src.push(s);
                }
            }
        }
        instructions.push(ins);
        at += 1 + len;
    }
    Ok(Shader { stage, major, minor, instructions, tokens: at })
}

/// (has a destination, number of sources) by opcode, for decoding; None
/// for opcodes outside shader models 2 and 3.
fn shape(opcode: u16) -> Option<(bool, u8)> {
    use op::*;
    Some(match opcode {
        NOP | RET | ENDLOOP | ENDREP | ELSE | ENDIF | BREAK | PHASE => (false, 0),
        MOV | RCP | RSQ | EXP | LOG | LIT | FRC | ABS | NRM | MOVA | EXPP | LOGP | DSX | DSY | SGN => (true, 1),
        ADD | SUB | MUL | DP3 | DP4 | MIN | MAX | SLT | SGE | DST | M4X4 | M4X3 | M3X4 | M3X3 | M3X2 | POW | CRS
        | BEM | SETP | TEX | TEXLDL => (true, 2),
        MAD | LRP | CND | CMP | DP2ADD | TEXLDD => (true, 3),
        SINCOS => (true, 3),
        CALL | LABEL | REP | IF => (false, 1),
        CALLNZ | LOOP | IFC | BREAKC => (false, 2),
        BREAKP => (false, 1),
        // texkill's operand is encoded as a destination (a write mask)
        TEXKILL | TEXCOORD => (true, 0),
        DCL | DEF | DEFI | DEFB => (true, 0),
        _ => return None,
    })
}

#[cfg(test)]
mod constant_extent_tests {
    use super::*;

    #[test]
    fn direct_matrix_and_relative_reads() {
        let direct = parse(&[0xfffe0300, 0x02000001, 0xe00f0000, 0xa0e40007, 0xffff]).unwrap();
        assert_eq!(direct.float_constant_count(), 8);
        let matrix = parse(&[0xfffe0300, 0x03000014, 0xe00f0000, 0x90e40000, 0xa0e40007, 0xffff]).unwrap();
        assert_eq!(matrix.float_constant_count(), 11);
        let relative = parse(&[0xfffe0300, 0x03000001, 0xe00f0000, 0xa0e42007, 0xb0000000, 0xffff]).unwrap();
        assert_eq!(relative.float_constant_count(), 256);
        let none = parse(&[0xffff0300, 0x02000001, 0x800f0800, 0x90e40000, 0xffff]).unwrap();
        assert_eq!(none.float_constant_count(), 0);
    }
}
