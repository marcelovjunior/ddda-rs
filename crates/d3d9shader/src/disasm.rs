//! Text in D3DDisassemble's format (instruction lines only; Microsoft's
//! comment header with the constant table is not reproduced). Every
//! formatting rule here is checked against D3DCompiler_47.dll on the
//! game's shaders (tests/disasm.rs).
use crate::{op, reg, Instruction, Param, Shader, Stage};

/// One line per instruction, indented by four spaces as Microsoft does,
/// starting with the version line; comments are skipped.
pub fn disassemble(s: &Shader) -> Vec<String> {
    let mut out = vec![format!(
        "    {}_{}_{}",
        if s.stage == Stage::Vertex { "vs" } else { "ps" },
        s.major,
        s.minor
    )];
    // flow-control bodies are indented two more spaces per level
    let mut depth = 0usize;
    for ins in &s.instructions {
        if ins.opcode == op::COMMENT {
            continue;
        }
        if matches!(ins.opcode, op::ENDLOOP | op::ENDREP | op::ENDIF | op::ELSE) {
            depth = depth.saturating_sub(1);
        }
        out.push(format!("    {}{}", "  ".repeat(depth), instruction(s, ins)));
        if matches!(ins.opcode, op::LOOP | op::REP | op::IF | op::IFC | op::ELSE) {
            depth += 1;
        }
    }
    out
}

fn name(opcode: u16, control: u8) -> &'static str {
    use op::*;
    match opcode {
        NOP => "nop",
        MOV => "mov",
        ADD => "add",
        SUB => "sub",
        MAD => "mad",
        MUL => "mul",
        RCP => "rcp",
        RSQ => "rsq",
        DP3 => "dp3",
        DP4 => "dp4",
        MIN => "min",
        MAX => "max",
        SLT => "slt",
        SGE => "sge",
        EXP => "exp",
        LOG => "log",
        LIT => "lit",
        DST => "dst",
        LRP => "lrp",
        FRC => "frc",
        M4X4 => "m4x4",
        M4X3 => "m4x3",
        M3X4 => "m3x4",
        M3X3 => "m3x3",
        M3X2 => "m3x2",
        CALL => "call",
        CALLNZ => "callnz",
        LOOP => "loop",
        RET => "ret",
        ENDLOOP => "endloop",
        LABEL => "label",
        POW => "pow",
        CRS => "crs",
        SGN => "sgn",
        ABS => "abs",
        NRM => "nrm",
        SINCOS => "sincos",
        REP => "rep",
        ENDREP => "endrep",
        IF => "if",
        IFC => "if",
        ELSE => "else",
        ENDIF => "endif",
        BREAK => "break",
        BREAKC => "break",
        MOVA => "mova",
        DEFB => "defb",
        DEFI => "defi",
        DEF => "def",
        TEXCOORD => "texcoord",
        TEXKILL => "texkill",
        TEX => match control {
            1 => "texldp",
            2 => "texldb",
            _ => "texld",
        },
        EXPP => "expp",
        LOGP => "logp",
        CND => "cnd",
        CMP => "cmp",
        BEM => "bem",
        DP2ADD => "dp2add",
        DSX => "dsx",
        DSY => "dsy",
        TEXLDD => "texldd",
        SETP => "setp",
        TEXLDL => "texldl",
        BREAKP => "breakp",
        PHASE => "phase",
        _ => "?",
    }
}

fn comparison(control: u8) -> &'static str {
    match control {
        1 => "_gt",
        2 => "_eq",
        3 => "_ge",
        4 => "_lt",
        5 => "_ne",
        6 => "_le",
        _ => "",
    }
}

fn instruction(s: &Shader, ins: &Instruction) -> String {
    let mut text = String::new();
    if ins.coissue {
        text.push('+');
    }
    if let Some(p) = &ins.predicate {
        text.push_str(&format!("({}) ", source(s, p)));
    }
    if ins.opcode == op::DCL {
        let d = ins.dst.as_ref().expect("dcl destination");
        return format!("{text}{} {}", declaration(s, ins.decl.unwrap_or(0), d), destination(s, d, true));
    }
    text.push_str(name(ins.opcode, ins.control));
    if matches!(ins.opcode, op::IFC | op::BREAKC | op::SETP) {
        text.push_str(comparison(ins.control));
    }
    if let Some(d) = &ins.dst {
        text.push_str(&result_modifiers(d));
    }
    let mut operands = Vec::new();
    if let Some(d) = &ins.dst {
        operands.push(destination(s, d, false));
    }
    match ins.opcode {
        op::DEF => operands.extend(ins.data.iter().map(|&t| float(f32::from_bits(t)))),
        op::DEFI => operands.extend(ins.data.iter().map(|&t| (t as i32).to_string())),
        op::DEFB => operands.extend(ins.data.iter().map(|&t| if t != 0 { "true".to_string() } else { "false".to_string() })),
        _ => operands.extend(ins.src.iter().map(|p| source(s, p))),
    }
    if operands.is_empty() {
        text
    } else {
        format!("{text} {}", operands.join(", "))
    }
}

fn result_modifiers(d: &Param) -> String {
    let mut m = String::new();
    if d.modifier & 1 != 0 {
        m.push_str("_sat");
    }
    if d.modifier & 2 != 0 {
        m.push_str("_pp");
    }
    if d.modifier & 4 != 0 {
        m.push_str("_centroid");
    }
    m
}

const USAGES: [&str; 14] = [
    "position", "blendweight", "blendindices", "normal", "psize", "texcoord", "tangent", "binormal", "tessfactor",
    "positiont", "color", "fog", "depth", "sample",
];

fn declaration(s: &Shader, usage: u32, d: &Param) -> String {
    if d.kind == reg::SAMPLER {
        return match (usage >> 27) & 0xF {
            2 => "dcl_2d",
            3 => "dcl_cube",
            4 => "dcl_volume",
            _ => "dcl",
        }
        .to_string();
    }
    let needs_usage = matches!(d.kind, reg::INPUT | reg::OUTPUT) && (s.major >= 3 || s.stage == Stage::Vertex);
    let mut text = if needs_usage {
        let u = USAGES.get((usage & 0x1F) as usize).copied().unwrap_or("?");
        let index = (usage >> 16) & 0xF;
        if index == 0 { format!("dcl_{u}") } else { format!("dcl_{u}{index}") }
    } else {
        "dcl".to_string()
    };
    text.push_str(&result_modifiers(d));
    text
}

fn register(s: &Shader, p: &Param) -> String {
    let n = p.index;
    match p.kind {
        reg::TEMP => format!("r{n}"),
        reg::INPUT => format!("v{n}"),
        reg::CONST => format!("c{n}"),
        reg::ADDR => {
            if s.stage == Stage::Vertex { format!("a{n}") } else { format!("t{n}") }
        }
        reg::RASTOUT => ["oPos", "oFog", "oPts"].get(n as usize).copied().unwrap_or("o?").to_string(),
        reg::ATTROUT => format!("oD{n}"),
        reg::OUTPUT => {
            if s.major >= 3 { format!("o{n}") } else { format!("oT{n}") }
        }
        reg::CONSTINT => format!("i{n}"),
        reg::COLOROUT => format!("oC{n}"),
        reg::DEPTHOUT => "oDepth".to_string(),
        reg::SAMPLER => format!("s{n}"),
        reg::CONSTBOOL => format!("b{n}"),
        reg::LOOP => "aL".to_string(),
        reg::MISCTYPE => if n == 0 { "vPos".to_string() } else { "vFace".to_string() },
        reg::LABEL => format!("l{n}"),
        reg::PREDICATE => format!("p{n}"),
        k => format!("?{k}:{n}"),
    }
}

fn write_mask(mask: u8) -> String {
    if mask == 0xF {
        return String::new();
    }
    let mut m = String::from(".");
    for (k, c) in "xyzw".chars().enumerate() {
        if mask & (1 << k) != 0 {
            m.push(c);
        }
    }
    m
}

fn destination(s: &Shader, d: &Param, _decl: bool) -> String {
    let mut text = register(s, d);
    if let Some(r) = &d.relative {
        text = format!("{}[{}]", text, source(s, r));
    }
    text.push_str(&write_mask(d.mask_or_swizzle));
    text
}

fn swizzle(sw: u8) -> String {
    if sw == 0xE4 {
        return String::new();
    }
    let comps: Vec<char> = (0..4).map(|k| b"xyzw"[((sw >> (2 * k)) & 3) as usize] as char).collect();
    // a replicate swizzle prints one component; any other all four
    if comps.iter().all(|&c| c == comps[0]) {
        format!(".{}", comps[0])
    } else {
        format!(".{}", comps.iter().collect::<String>())
    }
}

fn source(s: &Shader, p: &Param) -> String {
    let mut reg_text = register(s, p);
    if let Some(r) = &p.relative {
        // the register with its base index, then the address: c41[a0.x]
        reg_text = format!("{reg_text}[{}]", source(s, r));
    }
    let sw = swizzle(p.mask_or_swizzle);
    let body = format!("{reg_text}{sw}");
    match p.modifier {
        0 => body,
        1 => format!("-{body}"),
        2 => format!("{reg_text}_bias{sw}"),
        3 => format!("-{reg_text}_bias{sw}"),
        4 => format!("{reg_text}_bx2{sw}"),
        5 => format!("-{reg_text}_bx2{sw}"),
        6 => format!("1-{body}"),
        7 => format!("{reg_text}_x2{sw}"),
        8 => format!("-{reg_text}_x2{sw}"),
        9 => format!("{reg_text}_dz{sw}"),
        10 => format!("{reg_text}_dw{sw}"),
        11 => format!("{reg_text}_abs{sw}"),
        12 => format!("-{reg_text}_abs{sw}"),
        13 => format!("!{body}"),
        m => format!("?mod{m}:{body}"),
    }
}

/// A float as D3DDisassemble prints it: up to nine significant digits,
/// shortest form ("1", "0.5", "-0", "0.333333343").
pub fn float(v: f32) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let text = format!("{:.8e}", v);
    let (mantissa, exp) = text.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let (sign, digits) = match mantissa.strip_prefix('-') {
        Some(d) => ("-", d.replace('.', "")),
        None => ("", mantissa.replace('.', "")),
    };
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // C's %g: exponent form below 1e-4 and from 1e9
    if !(-4..9).contains(&exp) {
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() { String::new() } else { format!(".{rest}") };
        return format!("{sign}{first}{rest}e{}{:03}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    if exp < 0 {
        return format!("{sign}0.{}{}", "0".repeat((-exp - 1) as usize), digits);
    }
    let int_len = exp as usize + 1;
    if digits.len() <= int_len {
        format!("{sign}{}{}", digits, "0".repeat(int_len - digits.len()))
    } else {
        format!("{sign}{}.{}", &digits[..int_len], &digits[int_len..])
    }
}
