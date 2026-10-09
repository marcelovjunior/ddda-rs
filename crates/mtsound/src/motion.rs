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

#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceRequest {
    pub find_intersection: bool,
    /// Surface-bank type 0, actor common type 1, then bound banks 2..11.
    pub request_type: u32,
    pub requests: [u32; 32],
}

impl SurfaceRequest {
    // Translated from 0x00ce00e0 (DDDA.exe 2017-12-13 Steam).
    pub fn request(&self, surface: u32) -> u32 {
        self.requests.get(surface as usize).copied().unwrap_or(self.requests[0])
    }
}

#[derive(Debug, Clone)]
pub struct SurfaceRequests(pub Vec<Option<SurfaceRequest>>);

impl SurfaceRequests {
    /// rSoundAttributeSe XFS fields reflected by 0x00cdf460/0x00cdf760.
    /// Entries are indexed by the motion request's selected SeReqNo.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let x = mtxfs::parse(bytes).map_err(|e| e.to_string())?;
        let values = x.get(&x.root, "mpAttributeSeData").ok_or("missing surface sound rows")?;
        let mut rows = Vec::with_capacity(values.len());
        for value in values {
            let o = match value {
                Value::Object(Some(o)) => o,
                Value::Object(None) => { rows.push(None); continue; }
                _ => return Err("invalid surface sound row".into()),
            };
            let mut requests = [0; 32];
            for (n, value) in requests.iter_mut().enumerate() {
                *value = u32::try_from(integer(&x, o, &format!("SeReqID_{n:02}"))?)
                    .map_err(|_| "invalid surface sound request")?;
            }
            rows.push(Some(SurfaceRequest {
                find_intersection: boolean(&x, o, "UseFindIntersect")?,
                request_type: u32::try_from(integer(&x, o, "ReqType")?).map_err(|_| "invalid surface ReqType")?,
                requests,
            }));
        }
        Ok(Self(rows))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponBanks {
    pub common: String,
    pub specific: String,
}

/// Paths selected from the equipped weapon model ID, not its inventory ID.
/// Primary banks bind direct request types 3/4; secondary banks bind 5/6.
// Translated from 0x004b0ed0 / 0x004b11f0 (DDDA.exe 2017-12-13 Steam).
pub fn weapon_banks(model: u32, secondary: bool) -> Option<WeaponBanks> {
    let category = model >> 24;
    let name = match (secondary, category) {
        (false, 1) => "one", (false, 2) => "mac", (false, 3) => "two",
        (false, 4) => "dag", (false, 5 | 6) => "wan", (false, 7) => "war",
        (true, 8) => "sld", (true, 9) => "bld", (true, 10) => "bow",
        (true, 11) => "bbw", (true, 12) => "mbw",
        _ => return None,
    };
    let variant = match (model & 0x00ff_ffff) % 100 {
        12 => 100,
        13 => 200,
        15 if !secondary => 300,
        2 | 8 if !secondary && category == 7 => 2,
        _ => 0,
    };
    Some(WeaponBanks {
        common: format!(r"sound\se\wep\{name}_com\wep_{name}_com"),
        specific: format!(r"sound\se\wep\{name}{variant:03}\wep_{name}{variant:03}"),
    })
}

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
    fn weapon_bank_paths_use_model_category_and_variant_suffix() {
        assert_eq!(weapon_banks(0x0100_0000, false).unwrap().specific,
            r"sound\se\wep\one000\wep_one000");
        assert_eq!(weapon_banks(0x0400_0070, false).unwrap().specific,
            r"sound\se\wep\dag100\wep_dag100"); // 112 % 100 == 12
        assert_eq!(weapon_banks(0x0700_0008, false).unwrap().specific,
            r"sound\se\wep\war002\wep_war002");
        assert_eq!(weapon_banks(0x0600_000f, false).unwrap().specific,
            r"sound\se\wep\wan300\wep_wan300");
        assert_eq!(weapon_banks(0x0800_000f, true).unwrap().specific,
            r"sound\se\wep\sld000\wep_sld000");
        assert_eq!(weapon_banks(0x0a00_000d, true).unwrap().specific,
            r"sound\se\wep\bow200\wep_bow200");
        assert_eq!(weapon_banks(0x0100_0000, false).unwrap().common,
            r"sound\se\wep\one_com\wep_one_com");
        assert!(weapon_banks(u32::MAX, false).is_none());
        assert!(weapon_banks(0x0100_0000, true).is_none());
    }
    #[test]
    fn surface_lookup_preserves_sentinel_and_falls_back_only_for_unknown_surface() {
        let mut r = SurfaceRequest { find_intersection: true, request_type: 0, requests: [71; 32] };
        r.requests[2] = u32::MAX;
        r.requests[31] = 89;
        assert_eq!(r.request(2), u32::MAX);
        assert_eq!(r.request(31), 89);
        assert_eq!(r.request(32), 71);
        assert_eq!(r.request(u32::MAX), 71);
    }
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
