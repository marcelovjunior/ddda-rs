//! Reader for XFS, MT Framework's serialized object trees, as written by
//! Dragon's Dogma: Dark Arisen (version 0x0109). Parameter files, AI state
//! machines, layouts (`rLayout`), start positions and many other resources
//! are XFS.
//!
//! Layout (from Riftstone's `xfs.py`, MIT, which measured it over every
//! vanilla XFS file; checked here with `ddtool xfs-scan` on the install):
//!
//! ```text
//! header (0x14): "XFS\0", u16 0x0109, u16 class-specific version,
//!                u32 object count, u32 class count, u32 definition size
//! definitions:   u32 offset[class count] (from the block start)
//!                class: u32 type hash, u32 engine value, u32 property count,
//!                  property: u32 name offset, u8 type, u8 attr, u16 size,
//!                            16 zero bytes
//!                NUL-terminated names, padded to 4 bytes
//! objects:       pre-order tree; each object:
//!                  u16 class index << 1 | 1, u16 object number,
//!                  u32 size (from this field to the object's end),
//!                  per property: u32 count, then count values
//! ```
//!
//! Values: fixed-size numbers, vectors and shapes as stored; strings are
//! NUL-terminated; a class/classref value is a nested object or `FE FF 00
//! 00` for none; a resource reference is `02`, class name, NUL, path, NUL.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

mod classes;
pub use classes::CLASS_NAMES;

pub const MAGIC: [u8; 4] = *b"XFS\0";
pub const VERSION: u16 = 0x0109;
const HEADER: usize = 0x14;
const NULL_REF: [u8; 4] = [0xFE, 0xFF, 0x00, 0x00];
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "XFS at 0x{:x}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

fn err<T>(offset: usize, message: impl Into<String>) -> Result<T, Error> {
    Err(Error {
        offset,
        message: message.into(),
    })
}

/// MtType codes that occur in DDDA's XFS files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Type {
    Class,
    ClassRef,
    Bool,
    U8,
    U16,
    U32,
    U64,
    S8,
    S16,
    S32,
    S64,
    F32,
    F64,
    String,
    Matrix,
    Vector3,
    Vector4,
    Quaternion,
    CString,
    Float2,
    Float3,
    Float4,
    Sphere,
    Aabb,
    Obb,
    Cylinder,
    RangeF,
    RangeU16,
    HermiteCurve,
    Resource,
}

impl Type {
    pub fn from_code(c: u8) -> Option<Self> {
        use Type::*;
        Some(match c {
            0x01 => Class,
            0x02 => ClassRef,
            0x03 => Bool,
            0x04 => U8,
            0x05 => U16,
            0x06 => U32,
            0x07 => U64,
            0x08 => S8,
            0x09 => S16,
            0x0A => S32,
            0x0B => S64,
            0x0C => F32,
            0x0D => F64,
            0x0E => String,
            0x13 => Matrix,
            0x14 => Vector3,
            0x15 => Vector4,
            0x16 => Quaternion,
            0x20 => CString,
            0x22 => Float2,
            0x23 => Float3,
            0x24 => Float4,
            0x2D => Sphere,
            0x2F => Aabb,
            0x30 => Obb,
            0x31 => Cylinder,
            0x37 => RangeF,
            0x38 => RangeU16,
            0x39 => HermiteCurve,
            0x80 => Resource,
            _ => return None,
        })
    }

    /// Bytes per element for fixed-size types; `None` for variable-size ones.
    pub fn fixed_size(self) -> Option<usize> {
        use Type::*;
        Some(match self {
            Bool | U8 | S8 => 1,
            U16 | S16 => 2,
            U32 | S32 | F32 => 4,
            U64 | S64 | F64 | Float2 | RangeF => 8,
            RangeU16 => 4,
            Float3 => 12,
            Vector3 | Vector4 | Quaternion | Float4 | Sphere => 16,
            Aabb => 32,
            Cylinder => 48,
            Matrix | HermiteCurve => 64,
            Obb => 80,
            Class | ClassRef | String | CString | Resource => return None,
        })
    }

    fn is_float(self) -> bool {
        use Type::*;
        !matches!(
            self,
            Bool | U8
                | U16
                | U32
                | U64
                | S8
                | S16
                | S32
                | S64
                | RangeU16
                | Class
                | ClassRef
                | String
                | CString
                | Resource
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Property {
    pub name: String,
    pub ty: Type,
    pub attr: u8,
    pub size: u16,
}

impl Property {
    pub fn is_array(&self) -> bool {
        self.attr & 0x20 != 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassDef {
    /// `JAMCRC(class name) & 0x7FFF_FFFF`.
    pub hash: u32,
    pub engine_value: u32,
    pub properties: Vec<Property>,
}

impl ClassDef {
    /// The class name from the exe's MtDTI registry, if known.
    pub fn name(&self) -> Option<&'static str> {
        class_name(self.hash)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    U(u64),
    S(i64),
    /// Integers that come in fixed groups (`rangeu16`).
    U16s(Vec<u16>),
    /// One float, or the lanes of a vector, matrix or shape.
    F(Vec<f64>),
    Str(Vec<u8>),
    Object(Option<Box<Object>>),
    Resource {
        class: Vec<u8>,
        path: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub class: usize,
    pub number: u16,
    /// One entry per class property, in definition order; each holds the
    /// property's values (one for scalars, any number for arrays).
    pub fields: Vec<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Xfs {
    pub minor: u16,
    pub classes: Vec<ClassDef>,
    pub root: Object,
    pub object_count: u32,
}

impl Xfs {
    pub fn class_of(&self, o: &Object) -> &ClassDef {
        &self.classes[o.class]
    }

    /// The values of property `name` on object `o`, if the class has it.
    pub fn get<'a>(&self, o: &'a Object, name: &str) -> Option<&'a [Value]> {
        let i = self.classes[o.class]
            .properties
            .iter()
            .position(|p| p.name == name)?;
        Some(&o.fields[i])
    }

    /// Every object in the tree, pre-order (the file's numbering order).
    pub fn walk(&self) -> Vec<&Object> {
        fn rec<'a>(o: &'a Object, out: &mut Vec<&'a Object>) {
            out.push(o);
            for vals in &o.fields {
                for v in vals {
                    if let Value::Object(Some(c)) = v {
                        rec(c, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        rec(&self.root, &mut out);
        out
    }
}

/// Class name for an XFS type hash, from the exe's class registry.
pub fn class_name(hash: u32) -> Option<&'static str> {
    static TABLE: OnceLock<HashMap<u32, &'static str>> = OnceLock::new();
    TABLE
        .get_or_init(|| CLASS_NAMES.iter().map(|&n| (mtarc::type_hash(n), n)).collect())
        .get(&hash)
        .copied()
}

struct Reader<'a> {
    d: &'a [u8],
    classes: &'a [ClassDef],
    counter: u32,
}

fn u16_at(d: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([d[i], d[i + 1]])
}

fn u32_at(d: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(d[i..i + 4].try_into().unwrap())
}

fn cstr_end(d: &[u8], from: usize, end: usize, what: &str) -> Result<usize, Error> {
    d[from..end]
        .iter()
        .position(|&b| b == 0)
        .map(|p| from + p)
        .ok_or_else(|| Error {
            offset: from,
            message: format!("unterminated {what}"),
        })
}

impl Reader<'_> {
    fn object(&mut self, pos: usize, depth: usize) -> Result<(Object, usize), Error> {
        let d = self.d;
        if depth > MAX_DEPTH {
            return err(pos, "objects nested too deep");
        }
        if pos + 8 > d.len() {
            return err(pos, "object header runs past the end");
        }
        let tag = u16_at(d, pos);
        let number = u16_at(d, pos + 2);
        let size = u32_at(d, pos + 4) as usize;
        if tag & 1 == 0 {
            return err(pos, format!("object tag 0x{tag:04x} lacks its marker bit"));
        }
        let ci = usize::from(tag >> 1);
        if ci >= self.classes.len() {
            return err(pos, format!("object uses undeclared class {ci}"));
        }
        if u32::from(number) != self.counter & 0xFFFF {
            return err(
                pos + 2,
                format!("object number {number}, expected {}", self.counter),
            );
        }
        self.counter += 1;
        let end = pos + 4 + size;
        if size < 4 || end > d.len() {
            return err(pos + 4, format!("object size {size} runs past the end"));
        }
        let mut cur = pos + 8;
        let class = &self.classes[ci];
        let mut fields = Vec::with_capacity(class.properties.len());
        for p in &class.properties {
            if cur + 4 > end {
                return err(cur, format!("{}: count runs past the object", p.name));
            }
            let count = u32_at(d, cur) as usize;
            cur += 4;
            if count > 1 << 20 {
                return err(cur - 4, format!("{}: implausible count {count}", p.name));
            }
            let mut values = Vec::with_capacity(count.min(4096));
            if let Some(sz) = p.ty.fixed_size() {
                if cur + sz * count > end {
                    return err(cur, format!("{}: {count} values run past the object", p.name));
                }
                for _ in 0..count {
                    values.push(fixed_value(p.ty, &d[cur..cur + sz]));
                    cur += sz;
                }
            } else {
                match p.ty {
                    Type::String | Type::CString => {
                        for _ in 0..count {
                            let stop = cstr_end(d, cur, end, "string")?;
                            values.push(Value::Str(d[cur..stop].to_vec()));
                            cur = stop + 1;
                        }
                    }
                    Type::Class | Type::ClassRef => {
                        for _ in 0..count {
                            if d.get(cur..cur + 4) == Some(&NULL_REF[..]) {
                                values.push(Value::Object(None));
                                cur += 4;
                                continue;
                            }
                            let (child, next) = self.object(cur, depth + 1)?;
                            if next > end {
                                return err(cur, format!("{}: child runs past its parent", p.name));
                            }
                            values.push(Value::Object(Some(Box::new(child))));
                            cur = next;
                        }
                    }
                    Type::Resource => {
                        for _ in 0..count {
                            if cur >= end || d[cur] != 2 {
                                return err(cur, format!("{}: resource reference kind is not 2", p.name));
                            }
                            let s1 = cstr_end(d, cur + 1, end, "resource class")?;
                            let s2 = cstr_end(d, s1 + 1, end, "resource path")?;
                            values.push(Value::Resource {
                                class: d[cur + 1..s1].to_vec(),
                                path: d[s1 + 1..s2].to_vec(),
                            });
                            cur = s2 + 1;
                        }
                    }
                    _ => unreachable!("fixed-size types handled above"),
                }
            }
            fields.push(values);
        }
        if cur != end {
            return err(pos, format!("fields end at 0x{cur:x}, object claims 0x{end:x}"));
        }
        Ok((
            Object {
                class: ci,
                number,
                fields,
            },
            end,
        ))
    }
}

fn fixed_value(ty: Type, b: &[u8]) -> Value {
    match ty {
        Type::Bool => Value::Bool(b[0] != 0),
        Type::U8 => Value::U(u64::from(b[0])),
        Type::U16 => Value::U(u64::from(u16::from_le_bytes([b[0], b[1]]))),
        Type::U32 => Value::U(u64::from(u32_at(b, 0))),
        Type::U64 => Value::U(u64::from_le_bytes(b.try_into().unwrap())),
        Type::S8 => Value::S(i64::from(b[0] as i8)),
        Type::S16 => Value::S(i64::from(i16::from_le_bytes([b[0], b[1]]))),
        Type::S32 => Value::S(i64::from(u32_at(b, 0) as i32)),
        Type::S64 => Value::S(i64::from_le_bytes(b.try_into().unwrap())),
        Type::F64 => Value::F(vec![f64::from_le_bytes(b.try_into().unwrap())]),
        Type::RangeU16 => Value::U16s(vec![u16_at(b, 0), u16_at(b, 2)]),
        t if t.is_float() => Value::F(
            b.as_chunks::<4>()
                .0
                .iter()
                .map(|c| f64::from(f32::from_le_bytes(*c)))
                .collect(),
        ),
        _ => unreachable!(),
    }
}

/// Parses a complete XFS file.
pub fn parse(d: &[u8]) -> Result<Xfs, Error> {
    if d.len() < HEADER {
        return err(0, "shorter than the 20-byte header");
    }
    if d[0..4] != MAGIC {
        return err(0, "not an XFS file");
    }
    let version = u16_at(d, 4);
    if version != VERSION {
        return err(4, format!("version 0x{version:04x}, expected 0x{VERSION:04x}"));
    }
    let minor = u16_at(d, 6);
    let object_count = u32_at(d, 8);
    let nclasses = u32_at(d, 12) as usize;
    let defsize = u32_at(d, 16) as usize;
    let base = HEADER;
    let dend = base + defsize;
    if nclasses == 0 || nclasses > 4096 || defsize < 4 * nclasses || dend > d.len() {
        return err(
            12,
            format!("{nclasses} classes / {defsize}-byte definitions do not fit"),
        );
    }
    let mut classes = Vec::with_capacity(nclasses);
    for ci in 0..nclasses {
        let at = base + u32_at(d, base + 4 * ci) as usize;
        if at + 12 > dend {
            return err(base + 4 * ci, format!("class {ci} definition outside the block"));
        }
        let hash = u32_at(d, at);
        let engine_value = u32_at(d, at + 4);
        let nprops = u32_at(d, at + 8) as usize;
        if nprops > 4096 || at + 12 + 24 * nprops > dend {
            return err(at + 8, format!("class {ci}: {nprops} properties do not fit"));
        }
        let mut properties = Vec::with_capacity(nprops);
        for k in 0..nprops {
            let q = at + 12 + 24 * k;
            let name_at = base + u32_at(d, q) as usize;
            let code = d[q + 4];
            let attr = d[q + 5];
            let size = u16_at(d, q + 6);
            let Some(ty) = Type::from_code(code) else {
                return err(
                    q + 4,
                    format!("class 0x{hash:08x} property {k}: unknown type 0x{code:02x}"),
                );
            };
            if ty.fixed_size().is_some_and(|s| s != usize::from(size)) {
                return err(
                    q + 6,
                    format!("class 0x{hash:08x} property {k}: {ty:?} with size {size}"),
                );
            }
            if name_at >= dend {
                return err(q, "property name outside the definitions");
            }
            let stop = cstr_end(d, name_at, dend, "property name")?;
            let name = String::from_utf8_lossy(&d[name_at..stop]).into_owned();
            properties.push(Property { name, ty, attr, size });
        }
        classes.push(ClassDef {
            hash,
            engine_value,
            properties,
        });
    }
    let mut r = Reader {
        d,
        classes: &classes,
        counter: 0,
    };
    let (root, end) = r.object(dend, 0)?;
    if end != d.len() {
        return err(end, format!("{} bytes after the root object", d.len() - end));
    }
    if r.counter != object_count {
        return err(
            8,
            format!("header says {object_count} objects, tree has {}", r.counter),
        );
    }
    Ok(Xfs {
        minor,
        classes,
        root,
        object_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a tiny file: root class with a u32, a string and one child of
    /// a second class holding a vector3.
    fn sample() -> Vec<u8> {
        let mut defs = Vec::new();
        let names = b"mValue\0mName\0mChild\0mPos\0";
        // offsets: two classes; class0 has 3 props, class1 has 1
        let c0 = 8u32;
        let c1 = c0 + 12 + 24 * 3;
        let names_at = c1 + 12 + 24;
        defs.extend_from_slice(&c0.to_le_bytes());
        defs.extend_from_slice(&c1.to_le_bytes());
        let prop = |defs: &mut Vec<u8>, name_off: u32, code: u8, size: u16| {
            defs.extend_from_slice(&(names_at + name_off).to_le_bytes());
            defs.push(code);
            defs.push(0);
            defs.extend_from_slice(&size.to_le_bytes());
            defs.extend_from_slice(&[0; 16]);
        };
        defs.extend_from_slice(&mtarc::type_hash("rLayout").to_le_bytes());
        defs.extend_from_slice(&0u32.to_le_bytes());
        defs.extend_from_slice(&3u32.to_le_bytes());
        prop(&mut defs, 0, 0x06, 4);
        prop(&mut defs, 7, 0x0E, 4);
        prop(&mut defs, 13, 0x01, 4);
        defs.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        defs.extend_from_slice(&0u32.to_le_bytes());
        defs.extend_from_slice(&1u32.to_le_bytes());
        prop(&mut defs, 20, 0x14, 16);
        defs.extend_from_slice(names);
        while defs.len() % 4 != 0 {
            defs.push(0);
        }
        let mut child = vec![];
        child.extend_from_slice(&(1u16 << 1 | 1).to_le_bytes());
        child.extend_from_slice(&1u16.to_le_bytes());
        let body: Vec<u8> = [
            1u32.to_le_bytes().as_slice(),
            &[0, 0, 128, 63, 0, 0, 0, 64, 0, 0, 64, 64, 0, 0, 0, 0],
        ]
        .concat();
        child.extend_from_slice(&(4 + body.len() as u32).to_le_bytes());
        child.extend_from_slice(&body);
        let mut rbody = vec![];
        rbody.extend_from_slice(&1u32.to_le_bytes());
        rbody.extend_from_slice(&42u32.to_le_bytes());
        rbody.extend_from_slice(&1u32.to_le_bytes());
        rbody.extend_from_slice(b"hello\0");
        rbody.extend_from_slice(&1u32.to_le_bytes());
        rbody.extend_from_slice(&child);
        let mut root = vec![];
        root.extend_from_slice(&1u16.to_le_bytes());
        root.extend_from_slice(&0u16.to_le_bytes());
        root.extend_from_slice(&(4 + rbody.len() as u32).to_le_bytes());
        root.extend_from_slice(&rbody);
        let mut out = vec![];
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&7u16.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(defs.len() as u32).to_le_bytes());
        out.extend_from_slice(&defs);
        out.extend_from_slice(&root);
        out
    }

    #[test]
    fn parses_nested_tree() {
        let x = parse(&sample()).unwrap();
        assert_eq!(x.minor, 7);
        assert_eq!(x.classes[0].name(), Some("rLayout"));
        assert_eq!(x.get(&x.root, "mValue"), Some(&[Value::U(42)][..]));
        assert_eq!(
            x.get(&x.root, "mName"),
            Some(&[Value::Str(b"hello".to_vec())][..])
        );
        let objs = x.walk();
        assert_eq!(objs.len(), 2);
        assert_eq!(
            x.get(objs[1], "mPos"),
            Some(&[Value::F(vec![1.0, 2.0, 3.0, 0.0])][..])
        );
    }

    #[test]
    fn rejects_object_count_mismatch() {
        let mut b = sample();
        b[8] = 3;
        assert!(parse(&b).is_err());
    }
}
