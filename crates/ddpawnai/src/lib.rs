//! Pawn AI data of Dragon's Dogma: Dark Arisen: the priority tables that
//! choose what a pawn attends to (`rAIPriorityThink`, `AI\PrioThink\cmc`)
//! and the goal-planning sets that choose how it acts (`rAIGoalPlanning`,
//! `AI\Goap\Cmc\*`). Rules and addresses: `docs/PAWN_AI.md`.
//!
//! Field names in the data are the engine's own, often Japanese; the Rust
//! names translate them and the doc comments keep the originals.

pub mod commands;
pub mod chatter;
pub mod goap;
pub mod incl;
pub mod pawn;
pub mod plact;
pub mod prio;
pub mod think;

use mtxfs::{Object, Value, Xfs};

pub(crate) fn field<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a [Value], String> {
    x.get(o, name).ok_or_else(|| format!("{}: no field {name}", x.class_of(o).name().unwrap_or("?")))
}

pub(crate) fn int(x: &Xfs, o: &Object, name: &str) -> Result<i64, String> {
    match field(x, o, name)?.first() {
        Some(Value::U(u)) => Ok(*u as i64),
        Some(Value::S(s)) => Ok(*s),
        Some(Value::Bool(b)) => Ok(*b as i64),
        other => Err(format!("{name}: not an integer: {other:?}")),
    }
}

pub(crate) fn float(x: &Xfs, o: &Object, name: &str) -> Result<f32, String> {
    match field(x, o, name)?.first() {
        Some(Value::F(f)) if !f.is_empty() => Ok(f[0] as f32),
        other => Err(format!("{name}: not a float: {other:?}")),
    }
}

/// An integer or float field as f64 (some resources store the same
/// property with different types).
pub(crate) fn number(x: &Xfs, o: &Object, name: &str) -> Result<f64, String> {
    match field(x, o, name)?.first() {
        Some(Value::U(u)) => Ok(*u as f64),
        Some(Value::S(s)) => Ok(*s as f64),
        Some(Value::F(f)) if !f.is_empty() => Ok(f[0]),
        other => Err(format!("{name}: not a number: {other:?}")),
    }
}

pub(crate) fn string(x: &Xfs, o: &Object, name: &str) -> Result<String, String> {
    match field(x, o, name)?.first() {
        Some(Value::Str(s)) => Ok(String::from_utf8_lossy(s).into_owned()),
        other => Err(format!("{name}: not a string: {other:?}")),
    }
}

/// The objects of a property: a direct object list, or the elements of an
/// `MtArray` (`mpArray`) it wraps.
pub(crate) fn list<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<Vec<&'a Object>, String> {
    let mut out = Vec::new();
    for v in field(x, o, name)? {
        match v {
            Value::Object(Some(b)) => {
                if x.class_of(b).name() == Some("MtArray") {
                    for e in field(x, b, "mpArray")? {
                        match e {
                            Value::Object(Some(e)) => out.push(&**e),
                            Value::Object(None) => {}
                            other => return Err(format!("{name}: array element {other:?}")),
                        }
                    }
                } else {
                    out.push(&**b);
                }
            }
            Value::Object(None) => {}
            other => return Err(format!("{name}: not an object: {other:?}")),
        }
    }
    Ok(out)
}

pub(crate) fn class(x: &Xfs, o: &Object) -> String {
    x.class_of(o).name().map(str::to_string).unwrap_or_else(|| format!("{:#x}", x.class_of(o).hash))
}
