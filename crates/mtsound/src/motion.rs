//! Motion-event sound requests. Bank binding and surface queries are host inputs.
use std::collections::BTreeMap;
use mtxfs::{Object, Value, Xfs};

#[derive(Debug, Clone, PartialEq)]
pub struct MotionRequest {
    pub unique_id: u16,
    /// Direct bank selector; attribute requests use the surface table instead.
    pub request_type: u32,
    pub requests: [i16; 3],
    pub joint: i32,
    pub attached: bool,
    pub offset: [f32; 3],
    pub up_margin: f32,
    pub down_margin: f32,
    pub surface: bool,
}

impl MotionRequest {
    /// The host chooses the selector (including the player-state override).
    // Translated from 0x00c736f0 (DDDA.exe 2017-12-13 Steam).
    pub fn request(&self, selector: u32) -> i16 {
        self.requests.get(selector as usize).copied().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct MotionRequests(pub BTreeMap<u16, MotionRequest>);

impl MotionRequests {
    /// XFS field names are reflected by 0x00ce1250. The UniqueID map is
    /// built by 0x00ce10a0: later entries replace earlier duplicate IDs.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let x = mtxfs::parse(bytes).map_err(|e| e.to_string())?;
        let mut entries = BTreeMap::new();
        for o in x.walk() {
            if x.class_of(o).name() != Some("rSoundMotionSe::cSoundMotionSeData") { continue; }
            let unique_id = integer(&x, o, "UniqueID")?;
            // The original cannot build a usable map for negative signed IDs.
            let unique_id = u16::try_from(unique_id).ok().filter(|id| *id <= i16::MAX as u16)
                .ok_or("invalid motion sound UniqueID")?;
            let mut requests = [0; 3];
            for (n, value) in requests.iter_mut().enumerate() {
                *value = i16::try_from(integer(&x, o, &format!("SeReqNo_{}", n + 1))?)
                    .map_err(|_| "motion sound request outside i16")?;
            }
            let offset = match scalar(&x, o, "Offset")? {
                Value::F(v) if v.len() >= 3 => [v[0] as f32, v[1] as f32, v[2] as f32],
                _ => return Err("invalid motion sound Offset".into()),
            };
            entries.insert(unique_id, MotionRequest {
                unique_id,
                request_type: u32::try_from(integer(&x, o, "ReqType")?).map_err(|_| "invalid ReqType")?,
                requests,
                joint: i32::try_from(integer(&x, o, "SeJointID")?).map_err(|_| "invalid SeJointID")?,
                attached: boolean(&x, o, "SeJointAttachFlag")?,
                offset,
                up_margin: float(&x, o, "UpMarginY")?,
                down_margin: float(&x, o, "DownMarginY")?,
                surface: boolean(&x, o, "AttributeSeFlag")?,
            });
        }
        Ok(Self(entries))
    }

    /// UniqueID lookup, not row indexing (0x00c722d0).
    pub fn get(&self, id: u16) -> Option<&MotionRequest> { self.0.get(&id) }
}

fn scalar<'a>(x: &Xfs, o: &'a Object, field: &str) -> Result<&'a Value, String> {
    match x.get(o, field) {
        Some([value]) => Ok(value),
        _ => Err(format!("missing/non-scalar motion sound {field}")),
    }
}
fn integer(x: &Xfs, o: &Object, field: &str) -> Result<i64, String> {
    match scalar(x, o, field)? {
        Value::S(n) => Ok(*n),
        Value::U(n) => i64::try_from(*n).map_err(|_| format!("invalid {field}")),
        _ => Err(format!("non-integer {field}")),
    }
}
fn boolean(x: &Xfs, o: &Object, field: &str) -> Result<bool, String> {
    match scalar(x, o, field)? {
        Value::Bool(v) => Ok(*v),
        _ => Err(format!("non-boolean {field}")),
    }
}
fn float(x: &Xfs, o: &Object, field: &str) -> Result<f32, String> {
    match scalar(x, o, field)? {
        Value::F(v) if v.len() == 1 => Ok(v[0] as f32),
        _ => Err(format!("non-float {field}")),
    }
}

/// One motion layer's page-3 rising bits, remapped to sound UniqueIDs.
/// Track the previous bit word per actor and layer, never globally.
// Translated from 0x00c73630 / 0x00c721c0 / 0x00c722d0
// (DDDA.exe 2017-12-13 Steam).
pub fn rising_ids(previous: u32, current: u32, remap: &[u16; 32]) -> Vec<u16> {
    let rising = (previous ^ current) & current;
    (0..32).filter(|i| rising & (1 << i) != 0)
        .map(|i| remap[i]).filter(|id| *id != u16::MAX).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rising_edges_remap_and_do_not_repeat_held_bits() {
        let mut map = [u16::MAX; 32];
        map[0] = 42;
        map[31] = 7;
        assert_eq!(rising_ids(0, 0x8000_0003, &map), [42, 7]);
        assert!(rising_ids(0x8000_0003, 0x8000_0001, &map).is_empty());
        assert_eq!(rising_ids(0, 1, &map), [42]);
        let r = MotionRequest { unique_id: 42, request_type: 0, requests: [7, -1, 9],
            joint: -1, attached: false, offset: [0.0; 3], up_margin: 0.0,
            down_margin: 0.0, surface: false };
        assert_eq!((r.request(0), r.request(1), r.request(2), r.request(3)), (7, -1, 9, 0));
    }
}
