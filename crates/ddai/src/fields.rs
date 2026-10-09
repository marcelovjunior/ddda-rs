//! Named field access on `mtfsm::Record` (process parameters and condition
//! nodes keep their reflected field names; the names, not offsets, are the
//! interface). Mirrors `ddquest::order::Fields`.

use mtfsm::{Parameter, Record};
use mtxfs::Value;

pub trait Fields {
    fn value(&self, name: &str) -> Option<&Value>;
    fn object(&self, name: &str) -> Option<&Record>;
    fn u(&self, name: &str) -> Option<u64> {
        match self.value(name)? {
            Value::U(v) => Some(*v),
            Value::S(v) => u64::try_from(*v).ok(),
            _ => None,
        }
    }
    fn s(&self, name: &str) -> Option<i64> {
        match self.value(name)? {
            Value::S(v) => Some(*v),
            Value::U(v) => i64::try_from(*v).ok(),
            _ => None,
        }
    }
    fn b(&self, name: &str) -> Option<bool> {
        match self.value(name)? {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
    fn f(&self, name: &str) -> Option<f32> {
        match self.value(name)? {
            Value::F(v) => v.first().map(|&x| x as f32),
            _ => None,
        }
    }
    /// The first three lanes of a vector field.
    fn vec3(&self, name: &str) -> Option<[f32; 3]> {
        match self.value(name)? {
            Value::F(v) if v.len() >= 3 => Some([v[0] as f32, v[1] as f32, v[2] as f32]),
            _ => None,
        }
    }
    fn text(&self, name: &str) -> Option<String> {
        match self.value(name)? {
            Value::Str(s) => Some(String::from_utf8_lossy(s).into_owned()),
            _ => None,
        }
    }
}

impl Fields for Record {
    fn value(&self, name: &str) -> Option<&Value> {
        let (_, vs) = self.fields.iter().find(|(n, _)| n == name)?;
        match vs.first()? {
            Parameter::Scalar(v) => Some(v),
            Parameter::Object(_) => None,
        }
    }
    fn object(&self, name: &str) -> Option<&Record> {
        let (_, vs) = self.fields.iter().find(|(n, _)| n == name)?;
        match vs.first()? {
            Parameter::Object(Some(r)) => Some(r),
            _ => None,
        }
    }
}
