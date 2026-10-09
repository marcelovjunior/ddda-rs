//! Offline pawn data of Dragon's Dogma: Dark Arisen and the rules that turn
//! it into Rift pawns. Evidence and addresses: `docs/PAWNS.md`.
//!
//! All files live in `rom\etc\romNoraPawnData.arc` and are loaded together
//! by `cRomPawnManager` (`0x00796160`):
//!
//! | Resource | Class | Contents |
//! |---|---|---|
//! | `etc\editPattern\romNoraPawn00`..`32` | `rRomNoraPawn` | 33 level bands x 900 generated ("nora") pawn templates |
//! | `etc\editPattern\romPawnInfo` | `rRomPawnInfo` | 100 complete `cSAVE_DATA_CMC` pawns + 100 name numbers |
//! | `etc\editPattern\romPawnNameAlphabet` | `rRomPawnName` | Latin-alphabet names per base vocation and gender |
//!
//! The same loader also takes `etc\skill\skill`, the six `param\pl\level\Lv*`
//! tables, `param\pl\stamina\PlStaminaMax`, `AI\preset_charainfo` and
//! `AI\preset_study`, which the pawn builder `0x00795610` reads; those are
//! not typed here.

pub mod builder;
pub mod cmc;
pub mod rift;
pub mod tables;

pub use ddsave::pawn::{RomPawnId, NORA_LEVEL_BANDS};
use mtxfs::{Object, Value, Xfs};

/// Number of `romNoraPawnNN` files (`0x00796160` loops `< 0x21`).
pub const NORA_FILES: usize = 33;

fn err(what: &str) -> String {
    what.to_string()
}

fn field<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a [Value], String> {
    x.get(o, name).ok_or_else(|| format!("{}: no field {name}", x.class_of(o).name().unwrap_or("?")))
}

fn int_of(v: &Value, name: &str) -> Result<i64, String> {
    match v {
        Value::U(u) => Ok(*u as i64),
        Value::S(s) => Ok(*s),
        Value::Bool(b) => Ok(*b as i64),
        other => Err(format!("{name}: not an integer: {other:?}")),
    }
}

fn int(x: &Xfs, o: &Object, name: &str) -> Result<i64, String> {
    int_of(field(x, o, name)?.first().ok_or_else(|| format!("{name}: empty"))?, name)
}

fn ints(x: &Xfs, o: &Object, name: &str) -> Result<Vec<i64>, String> {
    field(x, o, name)?.iter().map(|v| int_of(v, name)).collect()
}

fn objects<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<Vec<&'a Object>, String> {
    field(x, o, name)?
        .iter()
        .map(|v| match v {
            Value::Object(Some(b)) => Ok(&**b),
            other => Err(format!("{name}: not an object: {other:?}")),
        })
        .collect()
}

fn object<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a Object, String> {
    objects(x, o, name)?.into_iter().next().ok_or_else(|| format!("{name}: empty"))
}

fn root_of<'a>(x: &'a Xfs, class: &str) -> Result<&'a Object, String> {
    match x.class_of(&x.root).name() {
        Some(n) if n == class => Ok(&x.root),
        other => Err(format!("root is {other:?}, expected {class}")),
    }
}

fn name_bytes(v: &[i64]) -> String {
    let bytes: Vec<u8> = v.iter().map(|&b| b as u8).take_while(|&b| b != 0).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Identity fields of a `cSAVE_DATA_EDIT` (the full appearance has 43
/// fields; only those needed to identify and name a pawn are typed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditId {
    /// `(u8*)mNameStr`: the Japanese name in the data files. Non-Japanese
    /// games replace it from [`PawnNames`] (`0x00795610`, `0x00795010`).
    pub name: String,
    /// 0 male, 1 female (matches the name list chosen; see docs/PAWNS.md).
    pub gender: u8,
    /// Index into `id\message\common\nickname_<lang>`.
    pub nickname: u32,
    pub voice: u8,
    pub hair: u8,
}

impl EditId {
    fn read(x: &Xfs, o: &Object) -> Result<EditId, String> {
        Ok(EditId {
            name: name_bytes(&ints(x, o, "(u8*)mNameStr")?),
            gender: int(x, o, "mGender")? as u8,
            nickname: int(x, o, "mNickname")? as u32,
            voice: int(x, o, "mVoice")? as u8,
            hair: int(x, o, "mHairNo")? as u8,
        })
    }
}

/// One generated pawn template (`rRomNoraPawn::cRomNoraPawn`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoraPawn {
    pub edit: EditId,
    /// The complete appearance (`mRomPawnEdit`), copied whole by the builder.
    pub edit_full: cmc::Edit,
    /// `mRomPawnWeightType`: row of `PlStaminaMax` used for base stamina
    /// (`0x00795610` indexes the stamina table with it).
    pub weight_type: u8,
    /// `mRomPawnJob`: 1 Fighter, 2 Strider, 3 Mage (only these occur).
    pub job: u8,
    /// `mRomPawnAiNo`: entry of `AI\preset_study` copied into the pawn's
    /// knowledge block (`0x00795610`).
    pub ai_no: i16,
    /// `mRomPawnCharaNo`: entry of `AI\preset_charainfo` (inclinations;
    /// applied through `0x00ca7320`, untraced in detail).
    pub chara_no: i16,
    /// `mRomPawnNameNo`: index into the [`PawnNames`] list for job and gender.
    pub name_no: u16,
    pub job_level: Vec<i8>,
    pub normal_skill: Vec<i8>,
    pub custom_skill: Vec<i16>,
    pub ability: Vec<i8>,
    /// `mRomPawnEquipItemId`: 11 item numbers, -1 empty.
    pub equip_item_id: Vec<i16>,
}

impl NoraPawn {
    fn read(x: &Xfs, o: &Object) -> Result<NoraPawn, String> {
        let small = |name: &str| -> Result<Vec<i64>, String> { ints(x, o, name) };
        Ok(NoraPawn {
            edit: EditId::read(x, object(x, o, "mRomPawnEdit")?)?,
            edit_full: read_edit(x, object(x, o, "mRomPawnEdit")?)?,
            weight_type: int(x, o, "mRomPawnWeightType")? as u8,
            job: int(x, o, "mRomPawnJob")? as u8,
            ai_no: int(x, o, "mRomPawnAiNo")? as i16,
            chara_no: int(x, o, "mRomPawnCharaNo")? as i16,
            name_no: int(x, o, "mRomPawnNameNo")? as u16,
            job_level: small("mRomPawnJobLevel")?.into_iter().map(|v| v as i8).collect(),
            normal_skill: small("mRomPawnNormalSkill")?.into_iter().map(|v| v as i8).collect(),
            custom_skill: small("mRomPawnCustomSkill")?.into_iter().map(|v| v as i16).collect(),
            ability: small("mRomPawnAbility")?.into_iter().map(|v| v as i8).collect(),
            equip_item_id: small("mRomPawnEquipItemId")?.into_iter().map(|v| v as i16).collect(),
        })
    }
}

fn read_edit(x: &Xfs, o: &Object) -> Result<cmc::Edit, String> {
    let mut e = cmc::Edit { name: [0; 25], nickname: int(x, o, "mNickname")? as u32, fields: [0; 41] };
    for (i, b) in ints(x, o, "(u8*)mNameStr")?.iter().take(25).enumerate() {
        e.name[i] = *b as u8;
    }
    for (i, f) in cmc::EDIT_U8_FIELDS.iter().enumerate() {
        e.fields[i] = int(x, o, f)? as u8;
    }
    Ok(e)
}

/// One `romNoraPawnNN` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoraFile {
    pub quality: i64,
    pub pawns: Vec<NoraPawn>,
}

impl NoraFile {
    pub fn parse(bytes: &[u8]) -> Result<NoraFile, String> {
        let x = mtxfs::parse(bytes).map_err(|e| format!("{e:?}"))?;
        let root = root_of(&x, "rRomNoraPawn")?;
        let array = object(&x, root, "mRomNoraPawn")?;
        let pawns = objects(&x, array, "mpArray")?
            .into_iter()
            .map(|o| NoraPawn::read(&x, o))
            .collect::<Result<_, _>>()?;
        Ok(NoraFile { quality: int(&x, root, "mQuality")?, pawns })
    }
}

/// Resource path of the nora file for a band.
pub fn nora_path(file: usize) -> String {
    format!(r"etc\editPattern\romNoraPawn{file:02}")
}

/// One complete built-in pawn of `rRomPawnInfo` (a `cSAVE_DATA_CMC`; only
/// identity, level and vocation are typed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RomPawn {
    pub edit: EditId,
    pub level: u8,
    pub job: u8,
    /// `mNameNoArray[i]`: replacement name number for non-Japanese games.
    pub name_no: u32,
}

/// `etc\editPattern\romPawnInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct RomPawnInfo {
    pub quality: i64,
    pub pawns: Vec<RomPawn>,
    /// The complete records, for the story stock builder.
    pub records: Vec<cmc::Cmc>,
}

impl RomPawnInfo {
    pub fn parse(bytes: &[u8]) -> Result<RomPawnInfo, String> {
        let x = mtxfs::parse(bytes).map_err(|e| format!("{e:?}"))?;
        let root = root_of(&x, "rRomPawnInfo")?;
        let names = ints(&x, root, "mNameNoArray")?;
        let cmcs = objects(&x, root, "mSaveDataCmcArray")?;
        if names.len() != cmcs.len() {
            return Err(err("mNameNoArray and mSaveDataCmcArray differ in length"));
        }
        let records = cmcs.iter().map(|c| cmc::Cmc::from_xfs(&x, c)).collect::<Result<Vec<_>, _>>()?;
        let pawns = cmcs
            .into_iter()
            .zip(names)
            .map(|(c, name_no)| {
                let param = object(&x, c, "mParam")?;
                Ok(RomPawn {
                    edit: EditId::read(&x, object(&x, c, "mEdit")?)?,
                    level: int(&x, param, "mLevel")? as u8,
                    job: int(&x, param, "mJob")? as u8,
                    name_no: name_no as u32,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(RomPawnInfo { quality: int(&x, root, "mQuality")?, pawns, records })
    }
}

/// `etc\editPattern\romPawnNameAlphabet`: six name lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PawnNames {
    /// `[mNameFighterMale, mNameFighterFemale, mNameStriderMale,
    /// mNameStriderFemale, mNameMageMale, mNameMageFemale]`.
    pub lists: Vec<Vec<String>>,
}

/// Property names of [`PawnNames::lists`], in order.
pub const NAME_LISTS: [&str; 6] = [
    "mNameFighterMale",
    "mNameFighterFemale",
    "mNameStriderMale",
    "mNameStriderFemale",
    "mNameMageMale",
    "mNameMageFemale",
];

impl PawnNames {
    pub fn parse(bytes: &[u8]) -> Result<PawnNames, String> {
        let x = mtxfs::parse(bytes).map_err(|e| format!("{e:?}"))?;
        let root = root_of(&x, "rRomPawnName")?;
        let lists = NAME_LISTS
            .iter()
            .map(|p| {
                field(&x, root, p)?
                    .iter()
                    .map(|v| match v {
                        Value::Str(s) => Ok(String::from_utf8_lossy(s).into_owned()),
                        other => Err(format!("{p}: not a string: {other:?}")),
                    })
                    .collect()
            })
            .collect::<Result<_, String>>()?;
        Ok(PawnNames { lists })
    }

    /// The Latin-alphabet name of a nora pawn.
    ///
    /// Translated from `0x00cdbf00` (DDDA.exe 2017-12-13 Steam): vocation 1
    /// or 7 (Fighter, Warrior) uses the Fighter lists, 2 or 8 (Strider,
    /// Ranger) the Strider lists, anything else the Mage lists; gender 0
    /// the male list, any other value the female one. The exe does not
    /// bound `name_no`; here an out-of-range number gives `None`. Matches
    /// every generated pawn in a real save (`crates/ddsave/tests/private_save.rs`).
    pub fn name(&self, job: u8, gender: u8, name_no: usize) -> Option<&str> {
        let base = match job {
            1 | 7 => 0,
            2 | 8 => 1,
            _ => 2,
        };
        let list = base * 2 + (gender != 0) as usize;
        self.lists.get(list)?.get(name_no).map(String::as_str)
    }
}
