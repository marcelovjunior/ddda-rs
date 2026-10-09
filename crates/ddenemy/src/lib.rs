//! Headless enemy parameter readers. Data values are not combat rules.
//! Reflected labels are retained; their gameplay interpretations are not
//! asserted unless documented in docs/ENEMIES.md.

mod region;
pub mod resources;
use mtxfs::{Object, Value, Xfs};
pub use region::{Region, RegionElement, RegionStatus};
use std::collections::BTreeMap;

fn one<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a Value, String> {
    match x.get(o, name) {
        Some([v]) => Ok(v),
        _ => Err(format!("{name}: expected one value")),
    }
}
fn float(x: &Xfs, o: &Object, name: &str) -> Result<f32, String> {
    match one(x, o, name)? {
        Value::F(v) if v.len() == 1 => Ok(v[0] as f32),
        _ => Err(format!("{name}: expected scalar float")),
    }
}
fn uint(x: &Xfs, o: &Object, name: &str) -> Result<u32, String> {
    match one(x, o, name)? {
        Value::U(v) => u32::try_from(*v).map_err(|_| format!("{name}: overflows u32")),
        _ => Err(format!("{name}: expected unsigned integer")),
    }
}
fn boolean(x: &Xfs, o: &Object, name: &str) -> Result<bool, String> {
    match one(x, o, name)? {
        Value::Bool(v) => Ok(*v),
        _ => Err(format!("{name}: expected bool")),
    }
}
fn string(x: &Xfs, o: &Object, name: &str) -> Result<Vec<u8>, String> {
    match one(x, o, name)? {
        Value::Str(v) => Ok(v.clone()),
        _ => Err(format!("{name}: expected string")),
    }
}
fn strings(x: &Xfs, o: &Object, name: &str) -> Result<Vec<Vec<u8>>, String> {
    x.get(o, name)
        .ok_or_else(|| format!("missing {name}"))?
        .iter()
        .map(|v| match v {
            Value::Str(v) => Ok(v.clone()),
            _ => Err(format!("{name}: expected string array")),
        })
        .collect()
}
fn signed(x: &Xfs, o: &Object, name: &str) -> Result<i32, String> {
    match one(x, o, name)? {
        Value::S(v) => i32::try_from(*v).map_err(|_| format!("{name}: overflows i32")),
        _ => Err(format!("{name}: expected signed integer")),
    }
}
fn child<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a Object, String> {
    match one(x, o, name)? {
        Value::Object(Some(v)) => Ok(v),
        _ => Err(format!("{name}: expected object")),
    }
}
fn children<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<Vec<Option<&'a Object>>, String> {
    x.get(o, name)
        .ok_or_else(|| format!("missing {name}"))?
        .iter()
        .map(|v| match v {
            Value::Object(v) => Ok(v.as_deref()),
            _ => Err(format!("{name}: expected object array")),
        })
        .collect()
}
fn root_class(x: &Xfs, name: &str) -> Result<(), String> {
    if x.class_of(&x.root).hash == mtarc::type_hash(name) {
        Ok(())
    } else {
        Err(format!("root class is not {name}"))
    }
}

/// English names below are translations of the serialized Japanese labels,
/// not traced gameplay semantics. Resistance values are raw, not percentages
/// or multipliers. All other fields remain available in the PRP's XFS tree.
#[derive(Debug, Clone, PartialEq)]
pub struct CommonStats {
    pub experience: u32,
    pub attack: f32,
    pub defense: f32,
    pub magic_attack: f32,
    pub magic_defense: f32,
    pub weight: f32,
    pub size: u32,
    pub scale: f32,
    pub human_enemy_hp: f32,
    pub flinch_guard: f32,
    pub knockback_guard: f32,
    /// Original labels beginning with 耐, including elemental/status entries.
    pub resistances: BTreeMap<String, f32>,
}
impl CommonStats {
    /// Read an explicitly selected common-parameter resource. This also
    /// supports child-unit paths, without widening the em#### identity type.
    /// The original reflected object is retained for additional parameters.
    pub fn load(vfs: &ddvfs::Vfs, path: &str) -> Result<(mtprp::PropParam, Self), String> {
        let bytes = vfs.read(path, mtarc::type_hash("rPropParam"))?;
        let prop = mtprp::parse(&bytes).map_err(|e| format!("{path}: {e}"))?;
        let stats = Self::from_prop(&prop).map_err(|e| format!("{path}: {e}"))?;
        Ok((prop, stats))
    }

    /// Common class 0x7e509fe9 only; other PRP classes are not enemy stats.
    pub fn from_prop(prop: &mtprp::PropParam) -> Result<Self, String> {
        if prop.class_hash != 0x7e50_9fe9 {
            return Err("not the common enemy stat class".into());
        }
        let x = &prop.object;
        let o = &x.root;
        let mut resistances = BTreeMap::new();
        for p in &x.class_of(o).properties {
            if p.name.starts_with('耐') {
                resistances.insert(p.name.clone(), float(x, o, &p.name)?);
            }
        }
        Ok(Self {
            experience: uint(x, o, "経験値")?,
            attack: float(x, o, "攻撃力")?,
            defense: float(x, o, "防御力")?,
            magic_attack: float(x, o, "魔法攻撃力")?,
            magic_defense: float(x, o, "魔法防御力")?,
            weight: float(x, o, "体重")?,
            size: uint(x, o, "大きさ")?,
            scale: float(x, o, "スケール値")?,
            human_enemy_hp: float(x, o, "人間敵 HP")?,
            flinch_guard: float(x, o, "のけぞりガード")?,
            knockback_guard: float(x, o, "ぶっとびガード")?,
            resistances,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StatusEntry {
    pub category: u32,
    pub is_overwrite: bool,
    pub overwrite_num: u32,
    pub timer: f32,
    pub param0: f32,
    pub param1: f32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct StatusParam {
    pub quality: u32,
    /// Null entries retain their slot. Category meanings and timer units unknown.
    pub entries: Vec<Option<StatusEntry>>,
    pub source: Xfs,
}
impl StatusParam {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let source = mtxfs::parse(data).map_err(|e| e.to_string())?;
        Self::from_xfs(source)
    }
    fn from_xfs(source: Xfs) -> Result<Self, String> {
        let x = &source;
        root_class(x, "rStatusParam")?;
        let quality = uint(x, &x.root, "mQuality")?;
        let entries = children(x, &x.root, "mArray")?
            .into_iter()
            .map(|o| {
                o.map(|o| {
                    Ok(StatusEntry {
                        category: uint(x, o, "mCategory")?,
                        is_overwrite: boolean(x, o, "mIsOverwrite")?,
                        overwrite_num: uint(x, o, "mOverwriteNum")?,
                        timer: float(x, o, "mTimer")?,
                        param0: float(x, o, "mParam0")?,
                        param1: float(x, o, "mParam1")?,
                    })
                })
                .transpose()
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            quality,
            entries,
            source,
        })
    }
}

/// Common shell fields, with subclass-specific fields retained in `source`.
/// Units/enums are intentionally not interpreted as simulation behavior.
#[derive(Debug, Clone, PartialEq)]
pub struct Shell {
    pub class_hash: u32,
    pub object_number: u16,
    pub axis: i32,
    pub set_type: i32,
    pub scale: f32,
    pub init_wait_frame: f32,
    pub is_time_die: bool,
    pub die_frame: f32,
    pub durable: f32,
    pub max_hit_count: i32,
    pub option: u32,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ShellGroup {
    pub effect_paths: Vec<Vec<u8>>,
    pub sound_paths: Vec<Vec<u8>>,
    pub sound_option: u32,
    pub shells: Vec<Option<Shell>>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ShellParamList {
    pub quality: u32,
    pub common_effect_path: Vec<u8>,
    pub common_sound_path: Vec<u8>,
    pub collision_path: Vec<u8>,
    pub groups: Vec<Option<ShellGroup>>,
    pub source: Xfs,
}
impl ShellParamList {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let source = mtxfs::parse(data).map_err(|e| e.to_string())?;
        Self::from_xfs(source)
    }
    fn from_xfs(source: Xfs) -> Result<Self, String> {
        let x = &source;
        root_class(x, "rShlParamList")?;
        let root = &x.root;
        let quality = uint(x, root, "mQuality")?;
        let common_effect_path = string(x, root, "mCmnEpvPath")?;
        let common_sound_path = string(x, root, "mCmnSePath")?;
        let collision_path = string(x, root, "mCollisionPath")?;
        let list = child(x, root, "mParamList")?;
        let groups = children(x, list, "mpArray")?
            .into_iter()
            .map(|g| {
                g.map(|g| {
                    let list = child(x, g, "mShlList")?;
                    let shells = children(x, list, "mpArray")?
                        .into_iter()
                        .map(|o| {
                            o.map(|o| {
                                Ok(Shell {
                                    class_hash: x.class_of(o).hash,
                                    object_number: o.number,
                                    axis: signed(x, o, "axis")?,
                                    set_type: signed(x, o, "shlSetType")?,
                                    scale: float(x, o, "scale")?,
                                    init_wait_frame: float(x, o, "initWaitFrame")?,
                                    is_time_die: boolean(x, o, "isTimeDie")?,
                                    die_frame: float(x, o, "dieFrame")?,
                                    durable: float(x, o, "durable")?,
                                    max_hit_count: signed(x, o, "maxHitCount")?,
                                    option: uint(x, o, "shlOption")?,
                                })
                            })
                            .transpose()
                        })
                        .collect::<Result<_, String>>()?;
                    Ok(ShellGroup {
                        effect_paths: strings(x, g, "epvPath")?,
                        sound_paths: strings(x, g, "sePath")?,
                        sound_option: uint(x, g, "seOption")?,
                        shells,
                    })
                })
                .transpose()
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            quality,
            common_effect_path,
            common_sound_path,
            collision_path,
            groups,
            source,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AdjustParam {
    pub values: Vec<f32>,
}
impl AdjustParam {
    /// Layout described in docs/FORMATS.md; values remain indexed and unlabelled.
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let mut c = region::Cursor { data, at: 0 };
        if c.u32()? != u32::from_le_bytes(*b"ajp\0") || c.u32()? != 0x0201 {
            return Err("unknown adjust header/version".into());
        }
        let count = c.count(4)?;
        let values = (0..count).map(|_| c.f32()).collect::<Result<_, _>>()?;
        c.finish()?;
        Ok(Self { values })
    }
}

/// A validated em#### identity, not an inferred species or text name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyId(String);
impl EnemyId {
    pub fn parse(id: &str) -> Result<Self, String> {
        if id.len() != 6 || !id.starts_with("em") || !id.as_bytes()[2..].iter().all(u8::is_ascii_digit) {
            return Err("enemy id must be em followed by four digits".into());
        }
        Ok(Self(id.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Explicit path loading avoids inventing links for shared shell/status files.
pub struct EnemyData {
    pub id: EnemyId,
    pub common: Option<(mtprp::PropParam, CommonStats)>,
    pub specific: Option<mtprp::PropParam>,
    pub regions: Option<RegionStatus>,
    pub adjust: Option<AdjustParam>,
    pub shells: Option<ShellParamList>,
}
impl EnemyData {
    /// Loads matching observed path conventions; absent resources stay absent.
    /// Does not resolve resource-list aliases, variants, or shared shell tables.
    /// VFS uses the first archive copy, whose engine precedence is unresolved.
    pub fn load(vfs: &ddvfs::Vfs, id: EnemyId) -> Result<Self, String> {
        fn load<T>(
            vfs: &ddvfs::Vfs,
            path: &str,
            class: &str,
            parse: impl FnOnce(&[u8]) -> Result<T, String>,
        ) -> Result<Option<T>, String> {
            let hash = mtarc::type_hash(class);
            if vfs.locate(path, hash).is_none() {
                return Ok(None);
            }
            parse(&vfs.read(path, hash)?)
                .map(Some)
                .map_err(|e| format!("{path}: {e}"))
        }
        let base = format!("charparam\\em\\{}", id.as_str());
        let common = load(vfs, &format!("{base}_cmn"), "rPropParam", |d| {
            let prop = mtprp::parse(d)?;
            let stats = CommonStats::from_prop(&prop)?;
            Ok((prop, stats))
        })?;
        let specific = load(vfs, &base, "rPropParam", mtprp::parse)?;
        let regions = load(vfs, &base, "rRegionStatus", RegionStatus::parse)?;
        let param = format!("param\\{}", id.as_str());
        let adjust = load(vfs, &param, "rAdjustParam", AdjustParam::parse)?;
        let shells = load(vfs, &param, "rShlParamList", ShellParamList::parse)?;
        Ok(Self {
            id,
            common,
            specific,
            regions,
            adjust,
            shells,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn class(name: &str, props: &[(&str, mtxfs::Type)]) -> mtxfs::ClassDef {
        mtxfs::ClassDef {
            hash: mtarc::type_hash(name),
            engine_value: 0,
            properties: props
                .iter()
                .map(|&(name, ty)| mtxfs::Property {
                    name: name.into(),
                    ty,
                    attr: 0,
                    size: ty.fixed_size().unwrap_or(4) as u16,
                })
                .collect(),
        }
    }
    fn object(class: usize, number: u16, fields: Vec<Vec<Value>>) -> Object {
        Object {
            class,
            number,
            fields,
        }
    }
    fn boxed(o: Object) -> Value {
        Value::Object(Some(Box::new(o)))
    }
    #[test]
    fn status_uses_names_and_keeps_null_slots() {
        use mtxfs::Type::*;
        let entry = object(
            1,
            1,
            vec![
                vec![Value::F(vec![2.5])],
                vec![Value::U(8)],
                vec![Value::Bool(true)],
                vec![Value::U(3)],
                vec![Value::F(vec![1.25])],
                vec![Value::F(vec![-4.])],
            ],
        );
        let x = Xfs {
            minor: 0,
            object_count: 2,
            classes: vec![
                class("rStatusParam", &[("mArray", ClassRef), ("mQuality", U32)]),
                class(
                    "cStatusParam",
                    &[
                        ("mTimer", F32),
                        ("mCategory", U32),
                        ("mIsOverwrite", Bool),
                        ("mOverwriteNum", U32),
                        ("mParam0", F32),
                        ("mParam1", F32),
                    ],
                ),
            ],
            root: object(
                0,
                0,
                vec![vec![Value::Object(None), boxed(entry)], vec![Value::U(2)]],
            ),
        };
        let s = StatusParam::from_xfs(x.clone()).unwrap();
        assert_eq!(s.quality, 2);
        assert_eq!(
            s.entries,
            vec![
                None,
                Some(StatusEntry {
                    category: 8,
                    is_overwrite: true,
                    overwrite_num: 3,
                    timer: 2.5,
                    param0: 1.25,
                    param1: -4.
                })
            ]
        );
        let mut bad = x.clone();
        bad.root.fields[1] = vec![Value::S(2)];
        assert!(StatusParam::from_xfs(bad).is_err());
        let mut bad = x;
        bad.classes[0].hash = 0;
        assert!(StatusParam::from_xfs(bad).is_err());
    }
    #[test]
    fn shell_keeps_group_path_arrays_and_null_slots() {
        use mtxfs::Type::*;
        let x = Xfs {
            minor: 1,
            object_count: 4,
            classes: vec![
                class(
                    "rShlParamList",
                    &[
                        ("mQuality", U32),
                        ("mParamList", Class),
                        ("mCmnEpvPath", String),
                        ("mCmnSePath", String),
                        ("mCollisionPath", String),
                    ],
                ),
                class("MtArray", &[("mpArray", ClassRef)]),
                class(
                    "cShlParam",
                    &[
                        ("epvPath", String),
                        ("sePath", String),
                        ("seOption", U32),
                        ("mShlList", Class),
                    ],
                ),
            ],
            root: object(
                0,
                0,
                vec![
                    vec![Value::U(1)],
                    vec![boxed(object(
                        1,
                        1,
                        vec![vec![
                            Value::Object(None),
                            boxed(object(
                                2,
                                2,
                                vec![
                                    vec![Value::Str(b"a".to_vec()), Value::Str(vec![])],
                                    vec![Value::Str(b"b".to_vec())],
                                    vec![Value::U(7)],
                                    vec![boxed(object(1, 3, vec![vec![Value::Object(None)]]))],
                                ],
                            )),
                        ]],
                    ))],
                    vec![Value::Str(vec![])],
                    vec![Value::Str(vec![])],
                    vec![Value::Str(vec![])],
                ],
            ),
        };
        let s = ShellParamList::from_xfs(x).unwrap();
        assert!(s.groups[0].is_none());
        let g = s.groups[1].as_ref().unwrap();
        assert_eq!(g.effect_paths, vec![b"a".to_vec(), vec![]]);
        assert_eq!(g.sound_paths, vec![b"b".to_vec()]);
        assert_eq!(g.sound_option, 7);
        assert_eq!(g.shells, vec![None]);
    }
    #[test]
    fn ids_do_not_accept_paths() {
        assert_eq!(EnemyId::parse("em0100").unwrap().as_str(), "em0100");
        for id in ["em100", "em12345", "em../x", "emABCD", "EM0100", "ém010"] {
            assert!(EnemyId::parse(id).is_err());
        }
    }
    #[test]
    fn adjust_preserves_float_bits_and_validates_length() {
        let mut b = b"ajp\0".to_vec();
        for n in [0x0201u32, 2, (-1.25f32).to_bits(), 0x7fc00001] {
            b.extend(n.to_le_bytes());
        }
        let p = AdjustParam::parse(&b).unwrap();
        assert_eq!(p.values[0], -1.25);
        assert_eq!(p.values[1].to_bits(), 0x7fc00001);
        for n in 0..b.len() {
            assert!(AdjustParam::parse(&b[..n]).is_err());
        }
        b.push(0);
        assert!(AdjustParam::parse(&b).is_err());
    }
}
