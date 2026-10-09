//! Condition evaluation (`rAIConditionTree`), as the game's work nodes do it.
//! Ported from `ddstory::cond` (claude-7); addresses unchanged.
//!
//! - An operation with no operands is true (0x0117c040 leaves its result
//!   word at 1); with one operand the operator gets (a, none); with more,
//!   every adjacent pair must hold (0x0117c040).
//! - Operators dispatch in 0x0117bd30: 1 truth, 2 not truth, 3/4 ==/!=,
//!   5/8 </>=, 7/6 >/<=, 9 all bits, 10 any bit, 16 and, 17 or; anything
//!   else is false.
//! - Values have the work node's type: bool, s32 and u32 (both held as
//!   `Int`; their comparisons are signed in those routines), f32.

use crate::fields::Fields;
use crate::Error;
use mtfsm::Expression;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f32),
}

/// A variable reference as stored in a VariableNode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Var<'a> {
    pub owner: &'a str,
    pub property: &'a str,
    /// `mIndex` when `mIsArray` is set.
    pub index: Option<u32>,
}

pub trait Vars {
    fn var(&self, v: &Var) -> Result<Value, Error>;
}

/// ftol: truncation toward zero (0x012f05c0).
fn trunc(f: f32) -> i64 {
    f.trunc() as i64
}

// Translated from 0x01179d50 (DDDA.exe 2017-12-13 Steam).
fn truth(a: Option<Value>) -> bool {
    match a {
        Some(Value::Bool(b)) => b,
        Some(Value::Int(i)) => i != 0,
        // Type 3 (f32) falls to the default: false.
        Some(Value::Float(_)) | None => false,
    }
}

// Translated from 0x0117ad10 (DDDA.exe 2017-12-13 Steam).
fn eq(a: Option<Value>, b: Option<Value>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Bool(x), Value::Int(y)) => x == (y != 0),
        (Value::Bool(_), Value::Float(_)) => false,
        // An int against a bool compares with -1/0 ((c != 1) - 1).
        (Value::Int(x), Value::Bool(y)) => x == if y { -1 } else { 0 },
        (Value::Int(x), Value::Int(y)) => x as u32 == y as u32,
        (Value::Int(x), Value::Float(y)) => x as u32 == trunc(y) as u32,
        (Value::Float(x), Value::Int(y)) => x == y as f32,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Float(_), Value::Bool(_)) => false,
    }
}

// Translated from 0x0117b200 (DDDA.exe 2017-12-13 Steam). Bools never compare.
fn lt(a: Option<Value>, b: Option<Value>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => (x as i32) < (y as i32),
        (Value::Int(x), Value::Float(y)) => (x as i32) < trunc(y) as i32,
        (Value::Float(x), Value::Int(y)) => x < y as i32 as f32,
        (Value::Float(x), Value::Float(y)) => x < y,
        _ => false,
    }
}

// Translated from 0x0117b590 (DDDA.exe 2017-12-13 Steam): the mirror of lt.
fn gt(a: Option<Value>, b: Option<Value>) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => (y as i32) < (x as i32),
        (Value::Int(x), Value::Float(y)) => (trunc(y) as i32) < x as i32,
        (Value::Float(x), Value::Int(y)) => (y as i32 as f32) < x,
        (Value::Float(x), Value::Float(y)) => y < x,
        _ => false,
    }
}

fn as_bits(v: Value) -> Option<u32> {
    match v {
        Value::Int(i) => Some(i as u32),
        _ => None,
    }
}

// Translated from 0x01179de0 (9, all bits) / 0x01179f80 (10, any bit):
// integers only; anything else is false.
fn bits(a: Option<Value>, b: Option<Value>, all: bool) -> bool {
    match (a.and_then(as_bits), b.and_then(as_bits)) {
        (Some(x), Some(y)) => {
            if all {
                x & y == y
            } else {
                x & y != 0
            }
        }
        _ => false,
    }
}

// Translated from 0x0117a0f0 (16) / 0x0117a3b0 (17): bool first is logical
// (the second as bool/int != 0); int first is bitwise; float first false.
fn andor(a: Option<Value>, b: Option<Value>, and: bool) -> bool {
    let (Some(a), Some(b)) = (a, b) else { return false };
    match a {
        Value::Bool(x) => {
            let y = match b {
                Value::Bool(y) => y,
                Value::Int(y) => y != 0,
                Value::Float(_) => return false,
            };
            if and {
                x && y
            } else {
                x || y
            }
        }
        Value::Int(x) => {
            let y = match b {
                Value::Int(y) => y as u32,
                Value::Bool(y) => y as u32,
                Value::Float(_) => return false,
            };
            if and {
                x as u32 & y != 0
            } else {
                x as u32 | y != 0
            }
        }
        Value::Float(_) => false,
    }
}

/// One operator over a pair (0x0117bd30).
// Translated from 0x0117bd30 (DDDA.exe 2017-12-13 Steam).
pub fn apply(op: u32, a: Option<Value>, b: Option<Value>) -> bool {
    match op {
        1 => truth(a),
        2 => !truth(a),
        3 => eq(a, b),
        4 => !eq(a, b),
        5 => lt(a, b),
        6 => !gt(a, b),
        7 => gt(a, b),
        8 => !lt(a, b),
        9 => bits(a, b, true),
        10 => bits(a, b, false),
        16 => andor(a, b, true),
        17 => andor(a, b, false),
        _ => false,
    }
}

/// Value of one node.
pub fn value(e: &Expression, vars: &dyn Vars) -> Result<Option<Value>, Error> {
    let r = &e.record;
    match r.class_name {
        Some("rAIConditionTree::OperationNode") => Ok(Some(Value::Bool(eval(e, vars)?))),
        Some("rAIConditionTree::ConstS32Node") => Ok(r.s("mValue").map(Value::Int)),
        Some("rAIConditionTree::ConstF32Node") => Ok(r.f("mValue").map(Value::Float)),
        Some("rAIConditionTree::VariableNode") => {
            let info = r.object("mVariable").ok_or_else(|| Error::Data("variable node without info".into()))?;
            let property = info.text("mPropertyName").unwrap_or_default();
            if r.b("mIsDynamicIndex").unwrap_or(false) || r.b("mUseEnumIndex").unwrap_or(false) {
                return Err(Error::Unimplemented(format!("dynamic/enum variable index for {property}")));
            }
            if r.b("mIsBitNo").unwrap_or(false) {
                return Err(Error::Unimplemented(format!("bit-number variable {property}")));
            }
            let owner = info.text("mOwnerName").unwrap_or_default();
            let index = if r.b("mIsArray").unwrap_or(false) { r.u("mIndex").map(|v| v as u32) } else { None };
            vars.var(&Var { owner: &owner, property: &property, index }).map(Some)
        }
        other => Err(Error::Unimplemented(format!("condition node {other:?}"))),
    }
}

/// Truth of an operation node (0x0117c040).
// Translated from 0x0117c040 (DDDA.exe 2017-12-13 Steam).
pub fn eval(e: &Expression, vars: &dyn Vars) -> Result<bool, Error> {
    let Some(op) = e.operator else {
        // A bare value node as a root: its truth.
        return Ok(truth(value(e, vars)?));
    };
    let kids: Vec<Option<Value>> = e
        .children
        .iter()
        .map(|c| match c {
            Some(c) => value(c, vars),
            None => Ok(None),
        })
        .collect::<Result<_, _>>()?;
    Ok(match kids.len() {
        0 => true,
        1 => apply(op, kids[0], None),
        _ => kids.windows(2).all(|w| apply(op, w[0], w[1])),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_and_types() {
        let i = |v| Some(Value::Int(v));
        let f = |v| Some(Value::Float(v));
        let b = |v| Some(Value::Bool(v));
        assert!(apply(1, b(true), None));
        assert!(!apply(1, f(1.0), None));
        assert!(apply(3, i(7), f(7.25)));
        assert!(!apply(3, f(7.25), i(7)));
        assert!(apply(7, f(60.5), f(60.0)));
        assert!(apply(8, i(3), i(3)));
        assert!(!apply(5, b(false), i(1)));
        assert!(apply(16, b(true), i(2)));
        assert!(apply(16, i(6), i(2)));
        assert!(!apply(16, f(1.0), b(true)));
        assert!(apply(9, i(6), i(2)) && !apply(9, i(4), i(6)));
        assert!(!apply(0, b(true), b(true)));
        assert!(!apply(3, i(1), b(true)));
        assert!(apply(3, i(-1), b(true)));
        // 6 is "not greater": <=.
        assert!(apply(6, f(200.0), f(200.0)) && !apply(6, f(200.5), f(200.0)));
    }
}
