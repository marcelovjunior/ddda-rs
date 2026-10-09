//! Strict, headless readers for DDDA player parameter XFS resources.
//! Field semantics and evidence are documented in docs/PARAMS.md.
//! Array order is preserved; these loaders do not assign gameplay meaning to indices.

use mtxfs::{Object, Type, Value, Xfs};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

fn schema(x: &Xfs, o: &Object, class: &str, fields: &[(&str, Type)]) -> Result<(), Error> {
    let c = x.class_of(o);
    if c.hash != mtarc::type_hash(class) {
        return Err(Error(format!("expected {class}, got {:?}", c.name())));
    }
    if c.properties.len() != fields.len() {
        return Err(Error(format!("{class}: unexpected field count")));
    }
    for &(name, ty) in fields {
        if c.properties
            .iter()
            .filter(|p| p.name == name && p.ty == ty)
            .count()
            != 1
        {
            return Err(Error(format!("{class}: missing, duplicate or mistyped {name}")));
        }
    }
    Ok(())
}
fn scalar<'a>(x: &Xfs, o: &'a Object, name: &str) -> Result<&'a Value, Error> {
    match x.get(o, name) {
        Some([v]) => Ok(v),
        _ => Err(Error(format!("{name}: expected one value"))),
    }
}
trait Field: Sized {
    fn read(v: &Value) -> Result<Self, Error>;
}
impl Field for f32 {
    fn read(v: &Value) -> Result<Self, Error> {
        if let Value::F(f) = v {
            if let [n] = f.as_slice() {
                return Ok(*n as f32);
            }
        }
        Err(Error("expected scalar float".into()))
    }
}
impl Field for [f32; 4] {
    fn read(v: &Value) -> Result<Self, Error> {
        if let Value::F(f) = v {
            if let [a, b, c, d] = f.as_slice() {
                return Ok([*a as f32, *b as f32, *c as f32, *d as f32]);
            }
        }
        Err(Error("expected four stored vector lanes".into()))
    }
}
macro_rules! integer_field {
    ($t:ty, $variant:ident) => {
        impl Field for $t {
            fn read(v: &Value) -> Result<Self, Error> {
                if let Value::$variant(n) = v {
                    return Self::try_from(*n).map_err(|_| Error("integer out of range".into()));
                }
                Err(Error("expected integer".into()))
            }
        }
    };
}
integer_field!(u16, U);
integer_field!(u32, U);
integer_field!(i32, S);

macro_rules! table {
    ($table:ident, $row:ident, $root:literal, $child:literal, {$($field:ident: $ty:ty => ($name:literal, $kind:ident)),+ $(,)?}) => {
        #[derive(Debug, Clone, PartialEq)]
        pub struct $row { $(pub $field: $ty),+ }
        #[derive(Debug, Clone, PartialEq)]
        pub struct $table {
            /// Serialized resource quality flag; not a gameplay tier.
            pub quality: u32,
            pub entries: Vec<$row>,
        }
        impl $table {
            pub const RESOURCE_CLASS: &'static str = $root;
            pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
                let x = mtxfs::parse(bytes).map_err(|e| Error(e.to_string()))?;
                schema(&x, &x.root, $root, &[("mQuality", Type::U32), ("mArray", Type::ClassRef)])?;
                let quality = u32::read(scalar(&x, &x.root, "mQuality")?)?;
                let entries = x.get(&x.root, "mArray").unwrap().iter().enumerate().map(|(index, v)| {
                    let Value::Object(Some(o)) = v else { return Err(Error(format!("mArray[{index}]: expected non-null object"))); };
                    schema(&x, o, $child, &[$(($name, Type::$kind)),+])?;
                    Ok($row { $($field: <$ty>::read(scalar(&x, o, $name)?)?),+ })
                }).collect::<Result<Vec<_>, Error>>()?;
                Ok(Self { quality, entries })
            }
        }
    }
}

table!(Stamina, StaminaEntry, "rPlStamina", "cPlStamina", {
    stamina: f32 => ("mStamina", F32),
    frame: f32 => ("mFrame", F32),
    motion_speed: f32 => ("mMotSpd", F32),
});

table!(Weight, WeightEntry, "rPlWeight", "cPlWeight", {
    weight: f32 => ("mWeight", F32),
});

table!(Level, LevelEntry, "rPlLevel", "cPlLevel", {
    level: u16 => ("mLevel", U16),
    hp_max: u16 => ("mHPMax", U16),
    stamina_max: u16 => ("mStaminaMax", U16),
    attack: u16 => ("mAttack", U16),
    defend: u16 => ("mDefend", U16),
    magic_attack: u16 => ("mMgcAttack", U16),
    magic_defend: u16 => ("mMgcDefend", U16),
    next_exp: u32 => ("mNextExp", U32),
});

table!(Exp, ExpEntry, "rPlExp", "cPlExp", {
    level: u32 => ("mLv", U32),
    rate: f32 => ("mRate", F32),
});

table!(JobLevel, JobLevelEntry, "rPlJobLevel", "cPlJobLevel", {
    level: u32 => ("mLevel", U32),
    next_exp: u32 => ("mNextExp", U32),
});

table!(Ability, AbilityEntry, "rPlAbilityParam", "cPlAbilityParam", {
    float_0: f32 => ("mFVal0", F32),
    float_1: f32 => ("mFVal1", F32),
    signed_0: i32 => ("mSVal0", S32),
    signed_1: i32 => ("mSVal1", S32),
});

table!(Defend, DefendEntry, "rPlDefendParam", "cPlDefendParam", {
    shrink_max: f32 => ("mShrinkMax", F32),
    shrink_speed: f32 => ("mShrinkSpeed", F32),
    blow_max: f32 => ("mBlowMax", F32),
    blow_speed: f32 => ("mBlowSpeed", F32),
});

table!(NeckPos, NeckPosEntry, "rPlNeckPos", "cPlNeckPos", {
    position: [f32; 4] => ("mPos", Vector3),
});

/// Supported resource classes, in stable inspection order.
pub const RESOURCE_CLASSES: &[&str] = &[
    "rPlStamina",
    "rPlWeight",
    "rPlLevel",
    "rPlExp",
    "rPlJobLevel",
    "rPlAbilityParam",
    "rPlDefendParam",
    "rPlNeckPos",
];

#[derive(Debug, Clone, PartialEq)]
pub enum Parameter {
    Stamina(Stamina),
    Weight(Weight),
    Level(Level),
    Exp(Exp),
    JobLevel(JobLevel),
    Ability(Ability),
    Defend(Defend),
    NeckPos(NeckPos),
}
impl Parameter {
    pub fn from_bytes(class: &str, bytes: &[u8]) -> Result<Self, Error> {
        match class {
            "rPlStamina" => Ok(Self::Stamina(Stamina::from_bytes(bytes)?)),
            "rPlWeight" => Ok(Self::Weight(Weight::from_bytes(bytes)?)),
            "rPlLevel" => Ok(Self::Level(Level::from_bytes(bytes)?)),
            "rPlExp" => Ok(Self::Exp(Exp::from_bytes(bytes)?)),
            "rPlJobLevel" => Ok(Self::JobLevel(JobLevel::from_bytes(bytes)?)),
            "rPlAbilityParam" => Ok(Self::Ability(Ability::from_bytes(bytes)?)),
            "rPlDefendParam" => Ok(Self::Defend(Defend::from_bytes(bytes)?)),
            "rPlNeckPos" => Ok(Self::NeckPos(NeckPos::from_bytes(bytes)?)),
            _ => Err(Error(format!("unsupported parameter class {class}"))),
        }
    }
}
