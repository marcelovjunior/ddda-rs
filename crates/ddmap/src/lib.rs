//! Headless map resource readers. No destination selection or global player.
//! Serialized names are retained without assigning untraced gameplay semantics.
use mtxfs::{Object, Value, Xfs};

pub type Result<T> = std::result::Result<T, String>;

fn class(x: &Xfs, o: &Object, name: &str) -> Result<()> {
    if x.class_of(o).hash == mtarc::type_hash(name) {
        Ok(())
    } else {
        Err(format!("expected {name}, got {:?}", x.class_of(o).name()))
    }
}
fn values<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a [Value]> {
    x.get(o, name).ok_or_else(|| format!("missing {name}"))
}
fn one<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a Value> {
    match values(x, o, name)? {
        [v] => Ok(v),
        _ => Err(format!("{name}: expected scalar")),
    }
}
fn uint(x: &Xfs, o: &Object, name: &str) -> Result<u32> {
    match one(x, o, name)? {
        Value::U(n) => u32::try_from(*n).map_err(|_| format!("{name}: overflow")),
        _ => Err(format!("{name}: expected unsigned integer")),
    }
}
fn int(x: &Xfs, o: &Object, name: &str) -> Result<i32> {
    match one(x, o, name)? {
        Value::S(n) => i32::try_from(*n).map_err(|_| format!("{name}: overflow")),
        _ => Err(format!("{name}: expected signed integer")),
    }
}
fn float(v: &Value) -> Result<f32> {
    match v {
        Value::F(f) if f.len() == 1 => Ok(f[0] as f32),
        _ => Err("expected scalar float".into()),
    }
}
fn vector(v: &Value) -> Result<[f32; 4]> {
    match v {
        Value::F(f) if f.len() == 4 => Ok([f[0] as f32, f[1] as f32, f[2] as f32, f[3] as f32]),
        _ => Err("expected four Vector3 storage lanes".into()),
    }
}
fn object(v: &Value) -> Result<&Object> {
    match v {
        Value::Object(Some(o)) => Ok(o),
        _ => Err("expected non-null object".into()),
    }
}
fn infos(x: &Xfs) -> Result<Vec<&Object>> {
    let a = object(one(x, &x.root, "InfoList")?)?;
    class(x, a, "MtArray")?;
    values(x, a, "mpArray")?.iter().map(object).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Judgment {
    pub command: u32,
    pub parameters: [u32; 4],
}
#[derive(Debug, Clone, PartialEq)]
pub struct Landmark {
    pub position_type: i32,
    pub position: [f32; 4],
    pub kind: u32,
    pub name_id: u32,
    pub next_stage: i32,
    pub next_icon_index: i32,
    pub floor: i32,
    pub lock_flag: u32,
    pub group: u32,
    pub id: u32,
    pub usage: u32,
    pub judgments: Vec<Judgment>,
}
impl Landmark {
    /// Resolve NameID in the caller-selected map_placelist language table.
    /// The shipped unnamed sentinel (u32::MAX) and invalid ids return None.
    pub fn name<'a>(&self, messages: &mtgmd::MessageFile<'a>) -> Option<&'a str> {
        messages.message(self.name_id)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct Landmarks {
    pub quality: u32,
    pub stage: u32,
    pub entries: Vec<Landmark>,
}
impl Landmarks {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let x = mtxfs::parse(bytes).map_err(|e| e.to_string())?;
        class(&x, &x.root, "rMapLndMarkPos")?;
        let entries = infos(&x)?
            .into_iter()
            .map(|o| {
                class(&x, o, "rMapLndMarkPos::Info")?;
                let judgments = values(&x, o, "mJCommand")?
                    .iter()
                    .map(|v| {
                        let j = object(v)?;
                        class(&x, j, "rMapLndMarkPos::cListJudgment")?;
                        Ok(Judgment {
                            command: uint(&x, j, "mCommand")?,
                            parameters: [
                                uint(&x, j, "mParam00")?,
                                uint(&x, j, "mParam01")?,
                                uint(&x, j, "mParam02")?,
                                uint(&x, j, "mParam03")?,
                            ],
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok(Landmark {
                    position_type: int(&x, o, "mPosType")?,
                    position: vector(one(&x, o, "Pos")?)?,
                    kind: uint(&x, o, "Type")?,
                    name_id: uint(&x, o, "NameID")?,
                    next_stage: int(&x, o, "NextStage")?,
                    next_icon_index: int(&x, o, "NextIconIndex")?,
                    floor: int(&x, o, "Floor")?,
                    lock_flag: uint(&x, o, "Lock Flag")?,
                    group: uint(&x, o, "Group")?,
                    id: uint(&x, o, "ID")?,
                    usage: uint(&x, o, "Usage")?,
                    judgments,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            quality: uint(&x, &x.root, "mQuality")?,
            stage: uint(&x, &x.root, "mStageNo")?,
            entries,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StartPosition {
    pub position: [f32; 4],
    pub angle: f32,
    /// Three serialized companion offsets; not a limit on simulation players.
    pub offset_positions: Vec<[f32; 4]>,
    pub offset_angles: Vec<f32>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct StartPositions {
    pub quality: u32,
    pub entries: Vec<StartPosition>,
}
impl StartPositions {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let x = mtxfs::parse(bytes).map_err(|e| e.to_string())?;
        class(&x, &x.root, "rStartPos")?;
        let entries = infos(&x)?
            .into_iter()
            .map(|o| {
                class(&x, o, "rStartPos::Info")?;
                let offset_positions = values(&x, o, "OfsPos")?
                    .iter()
                    .map(vector)
                    .collect::<Result<Vec<_>>>()?;
                let offset_angles = values(&x, o, "OfsAng")?
                    .iter()
                    .map(float)
                    .collect::<Result<Vec<_>>>()?;
                // Reflection 0x00ce8740 (DDDA.exe 2017-12-13 Steam) declares both arrays as 3.
                if offset_positions.len() != 3 || offset_angles.len() != 3 {
                    return Err("start offsets must each have three serialized entries".into());
                }
                Ok(StartPosition {
                    position: vector(one(&x, o, "Pos")?)?,
                    angle: float(one(&x, o, "Ang")?)?,
                    offset_positions,
                    offset_angles,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            quality: uint(&x, &x.root, "mQuality")?,
            entries,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaceName {
    pub position: [f32; 3],
    pub radius: f32,
    pub message_number: u16,
    pub check_number: i16,
    pub game_state_flag: i16,
    pub angle: f32,
    pub range: f32,
}
impl PlaceName {
    /// Resolve the stored message number in map_placelist, retaining empty text.
    pub fn name<'a>(&self, messages: &mtgmd::MessageFile<'a>) -> Option<&'a str> {
        messages.message(self.message_number as u32)
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct PlaceNames {
    pub entries: Vec<PlaceName>,
}
impl PlaceNames {
    /// Translated from 0x00ce7e80 (DDDA.exe 2017-12-13 Steam).
    /// Names/types corroborated by reflection at 0x00ce7ba0.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 12 || &bytes[..4] != b"spn\0" {
            return Err("missing SPN header".into());
        }
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        if word(4) != 0x30 {
            return Err("unsupported SPN version".into());
        }
        let count = word(8) as usize;
        if count != (bytes.len() - 12) / 30 || (bytes.len() - 12) % 30 != 0 {
            return Err("SPN record count/length mismatch".into());
        }
        let entries = bytes[12..]
            .chunks_exact(30)
            .map(|r| {
                let f = |at| f32::from_le_bytes(r[at..at + 4].try_into().unwrap());
                let u = |at| u16::from_le_bytes(r[at..at + 2].try_into().unwrap());
                PlaceName {
                    position: [f(0), f(4), f(8)],
                    radius: f(12),
                    message_number: u(16),
                    check_number: u(18) as i16,
                    game_state_flag: u(20) as i16,
                    angle: f(22),
                    range: f(26),
                }
            })
            .collect();
        Ok(Self { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = b"spn\0".to_vec();
        b.extend(0x30u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
        for f in [1f32, -2., 3., 4.] {
            b.extend(f.to_le_bytes());
        }
        for n in [5u16, 0xffff, 6] {
            b.extend(n.to_le_bytes());
        }
        for f in [7f32, 8.] {
            b.extend(f.to_le_bytes());
        }
        b
    }
    #[test]
    fn place_name_disk_order() {
        assert_eq!(
            PlaceNames::parse(&fixture()).unwrap().entries,
            [PlaceName {
                position: [1., -2., 3.],
                radius: 4.,
                message_number: 5,
                check_number: -1,
                game_state_flag: 6,
                angle: 7.,
                range: 8.
            }]
        );
    }
    #[test]
    fn rejects_malformed_places() {
        let b = fixture();
        for end in 0..b.len() {
            assert!(PlaceNames::parse(&b[..end]).is_err());
        }
        for offset in [0, 4, 8] {
            let mut bad = b.clone();
            bad[offset] ^= 1;
            assert!(PlaceNames::parse(&bad).is_err());
        }
        let mut bad = b;
        bad.push(0);
        assert!(PlaceNames::parse(&bad).is_err());
    }
}
