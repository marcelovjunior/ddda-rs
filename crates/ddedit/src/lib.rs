//! Strict saved appearance decoding and bounded customization evidence helpers.
//! No global actor, editor UI, identity logging or process access.
pub use ddpawn::cmc::{Edit, EDIT_U8_FIELDS};
use ddsave::xml::Node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

fn field<'d, 'a>(n: Node<'d, 'a>, name: &str) -> Result<Node<'d, 'a>, Error> {
    let mut found = n.children().filter(|c| c.name().as_deref() == Some(name));
    let first = found.next().ok_or_else(|| Error(format!("missing {name}")))?;
    if found.next().is_some() {
        return Err(Error(format!("duplicate {name}")));
    }
    Ok(first)
}

/// Decode all 41 appearance bytes, nickname and the 25-byte name storage.
/// Schema: cSAVE_DATA_EDIT reflection 0x00787010 (see CHARA_EDIT.md).
/// Reject malformed present fields; unknown fields stay in the source document.
pub fn read_edit(n: Node<'_, '_>) -> Result<Edit, Error> {
    if n.tag() != "class" || n.ty().as_deref() != Some("cSAVE_DATA_EDIT") {
        return Err(Error("wrong edit class".into()));
    }
    for name in EDIT_U8_FIELDS {
        let v = field(n, name)?;
        if v.tag() != "u8" || v.children().next().is_some() || v.parse::<u8>().is_none() {
            return Err(Error(format!("invalid {name}")));
        }
    }
    let nickname = field(n, "mNickname")?;
    if nickname.tag() != "u32" || nickname.children().next().is_some() || nickname.parse::<u32>().is_none() {
        return Err(Error("invalid mNickname".into()));
    }
    let name = field(n, "(u8*)mNameStr")?;
    if name.tag() != "array" || name.ty().as_deref() != Some("u8") || name.count() != Some(25) {
        return Err(Error("invalid name storage shape".into()));
    }
    let mut count = 0;
    for byte in name.children() {
        if count == 25
            || byte.tag() != "u8"
            || byte.children().next().is_some()
            || byte.parse::<u8>().is_none()
        {
            return Err(Error("invalid name storage element".into()));
        }
        count += 1;
    }
    if count != 25 {
        return Err(Error("truncated name storage".into()));
    }
    ddpawn::cmc::Cmc::from_save_edit(n).ok_or_else(|| Error("invalid edit".into()))
}

/// The saved preset numbers are selectors, not body dimensions or slider ratios.
/// Translated from 0x00780fa0 (DDDA.exe 2017-12-13 Steam).
pub fn preset_offset(edit: &Edit) -> u32 {
    if edit.get("mGender") == Some(1) && edit.get("mHumanBodyAll").unwrap() < 200 {
        100
    } else {
        0
    }
}

/// Load the five body presets through the caller's resource source. Missing
/// presets are reported rather than silently imitating the engine reset path.
/// Reuses ddlook's source-backed preset composition; its neutral nine measurement
/// floats are NOT a reconstruction of height/weight (see CHARA_EDIT.md).
pub fn body_presets<S: ddlook::Source + ?Sized>(
    edit: &Edit,
    src: &S,
) -> Result<ddlook::morph::HumanEdit, Error> {
    let mut human = ddlook::morph::HumanEdit::neutral();
    for (part, (name, field)) in ddlook::morph::PRESET_PARTS.iter().enumerate() {
        let id = u32::from(edit.get(field).unwrap()) + preset_offset(edit);
        let path = format!("edit\\parts\\{name}\\{name}_{id:03}");
        let bytes = src
            .resource(&path, "rHumanPartsEdit")
            .map_err(|e| Error(e.to_string()))?
            .ok_or_else(|| Error(format!("missing preset {path}")))?;
        let preset = ddlook::morph::HumanParts::parse(&bytes).map_err(|e| Error(e.to_string()))?;
        human.apply_preset(part, Some(&preset));
    }
    Ok(human)
}

/// Ordinary enabled horizontal root-motion multiplier; not sprint speed.
/// Translated from 0x0081c170 (DDDA.exe 2017-12-13 Steam).
/// 0x0081c330 writes index[joint] at +0x34+2*id: +0x4e/50 are ids13/14.
/// Inputs must be live/derived active uCnsEdit mScale.y and owner model scale.y.
pub fn ground_scale(owner_y: f32, joint_13_y: f32, joint_14_y: f32) -> Option<f32> {
    if [owner_y, joint_13_y, joint_14_y].iter().any(|v| !v.is_finite()) {
        return None;
    }
    let value = joint_14_y * joint_13_y * owner_y;
    value.is_finite().then_some(value)
}

/// Body class from caller-loaded PlEditWeight values; the first row is not a
/// class boundary. Equality belongs to the higher class. No embedded data table.
/// Translated from 0x00b81120 (DDDA.exe 2017-12-13 Steam).
pub fn body_class(weight: f32, rows: &[f32; 5]) -> Option<usize> {
    if !weight.is_finite() || rows.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(if rows[4] <= weight {
        4
    } else if rows[3] <= weight {
        3
    } else if weight < rows[2] {
        if weight < rows[1] {
            0
        } else {
            1
        }
    } else {
        2
    })
}

/// Body weight from already computed runtime edit measurements. `human` holds
/// rHumanEdit +0x68/+0x6c/+0x70, `body` holds rBodyEdit +0x60/+0x64;
/// `bust` is cPlayerInfo +0x108, NOT the saved 0..4 selector.
/// Missing human resource returns the original 76 fallback. The resource
/// measurement producer 0x00cbbf00 is not implemented by this function.
/// Translated from 0x00780ea0 (DDDA.exe 2017-12-13 Steam), preserving SSE order
/// and CVTTSS2SI truncation. Nonfinite/out-of-i32 inputs return None.
pub fn body_weight(human: Option<[f32; 3]>, body: Option<[f32; 2]>, female: bool, bust: f32) -> Option<f32> {
    let Some([height, second, third]) = human else {
        return Some(76.);
    };
    if [height, second, third].iter().any(|x| !x.is_finite()) {
        return None;
    }
    let height = height * 1.8;
    let mut weight = ((third + second) * 0.526_315_8) * ((height * height) * 22.5);
    if let Some([slender, muscle]) = body {
        if !slender.is_finite() || !muscle.is_finite() || (female && !bust.is_finite()) {
            return None;
        }
        weight *= ((slender * 0.01) * 1.666_666_6) * 0.24 + 1.;
        weight *= (muscle * 0.01) * 0.08 + 1.;
        if female {
            weight *= bust * 0.02 + 1.;
        }
    }
    if !weight.is_finite() || !(-2147483648.0..2147483648.0).contains(&weight) {
        return None;
    }
    Some((weight as i32) as f32)
}
