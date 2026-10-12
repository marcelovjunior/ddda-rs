//! The constant table a shader carries in a comment block (FOURCC "CTAB",
//! d3dx9shader.h D3DXSHADER_CONSTANTTABLE) and the reflection D3DX
//! derives from it: what D3DXGetShaderConstantTableEx's ID3DXConstantTable
//! answers (GetDesc, GetConstant, GetConstantDesc) and what
//! D3DXGetShaderOutputSemantics answers. The game calls these when it links
//! a shader pair (0x00E10690). Checked against d3dx9_43.dll on every game
//! shader (crates/d3dxref).
use crate::{op, reg, Shader};

/// "CTAB"
pub const CTAB: u32 = 0x4241_5443;

/// The table's description (D3DXCONSTANTTABLE_DESC) and its top-level
/// constants.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    /// the CTAB block's bytes (after the FOURCC); names and default values
    /// are offsets into it
    pub data: Vec<u8>,
    pub creator: String,
    /// offset of the creator string in `data`
    pub creator_offset: u32,
    /// the shader version token
    pub version: u32,
    pub flags: u32,
    pub constants: Vec<Constant>,
}

/// One top-level constant as D3DXCONSTANT_DESC describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Constant {
    pub name: String,
    /// offset of the name in the table's data
    pub name_offset: u32,
    /// D3DXREGISTER_SET: 0 bool, 1 int4, 2 float4, 3 sampler
    pub register_set: u32,
    pub register_index: u32,
    pub register_count: u32,
    /// D3DXPARAMETER_CLASS
    pub class: u32,
    /// D3DXPARAMETER_TYPE
    pub ty: u32,
    pub rows: u32,
    pub columns: u32,
    pub elements: u32,
    pub struct_members: u32,
    /// size of the constant's data in bytes
    pub bytes: u32,
    /// offset of the default value in the table's data (0: none)
    pub default_offset: u32,
}

fn u16_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u16::from_le_bytes([*d.get(o)?, *d.get(o + 1)?]) as u32)
}
fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*d.get(o)?, *d.get(o + 1)?, *d.get(o + 2)?, *d.get(o + 3)?]))
}
fn string_at(d: &[u8], o: usize) -> Option<String> {
    let s = d.get(o..)?;
    let end = s.iter().position(|&b| b == 0)?;
    Some(String::from_utf8_lossy(&s[..end]).into_owned())
}

/// D3DXPARAMETER_CLASS values (d3dx9shader.h).
pub const CLASS_STRUCT: u32 = 5;
/// D3DXPARAMETER_TYPE values for samplers (SAMPLER .. SAMPLERCUBE).
const SAMPLER_TYPES: std::ops::RangeInclusive<u32> = 10..=14;

/// The data size of a type (D3DXSHADER_TYPEINFO at `type_offset`): four
/// bytes per component, times the elements; a structure is its members'
/// sizes added. Samplers count four bytes each.
fn type_bytes(d: &[u8], type_offset: usize) -> Option<u32> {
    let class = u16_at(d, type_offset)?;
    let ty = u16_at(d, type_offset + 2)?;
    let (rows, columns) = (u16_at(d, type_offset + 4)?, u16_at(d, type_offset + 6)?);
    let elements = u16_at(d, type_offset + 8)?.max(1);
    let members = u16_at(d, type_offset + 10)?;
    let member_info = u32_at(d, type_offset + 12)? as usize;
    let one = if class == CLASS_STRUCT {
        let mut sum = 0;
        for k in 0..members as usize {
            // D3DXSHADER_STRUCTMEMBERINFO: name offset, type offset
            sum += type_bytes(d, u32_at(d, member_info + 8 * k + 4)? as usize)?;
        }
        sum
    } else if SAMPLER_TYPES.contains(&ty) {
        4
    } else {
        4 * rows * columns
    };
    Some(one * elements)
}

/// The constant table of a shader, or None without one.
pub fn table(s: &Shader) -> Option<Table> {
    let comment = s.instructions.iter().find(|i| i.opcode == op::COMMENT && i.data.first() == Some(&CTAB))?;
    let data: Vec<u8> = comment.data[1..].iter().flat_map(|t| t.to_le_bytes()).collect();
    let d = &data;
    // D3DXSHADER_CONSTANTTABLE: Size, Creator, Version, Constants,
    // ConstantInfo, Flags, Target
    let creator_offset = u32_at(d, 4)?;
    let version = u32_at(d, 8)?;
    let count = u32_at(d, 12)?;
    let info = u32_at(d, 16)? as usize;
    let flags = u32_at(d, 20)?;
    let mut constants = Vec::new();
    for k in 0..count as usize {
        // D3DXSHADER_CONSTANTINFO: Name, RegisterSet (u16), RegisterIndex
        // (u16), RegisterCount (u16), Reserved, TypeInfo, DefaultValue
        let c = info + 20 * k;
        let name_offset = u32_at(d, c)?;
        let type_offset = u32_at(d, c + 12)? as usize;
        constants.push(Constant {
            name: string_at(d, name_offset as usize)?,
            name_offset,
            register_set: u16_at(d, c + 4)?,
            register_index: u16_at(d, c + 6)?,
            register_count: u16_at(d, c + 8)?,
            class: u16_at(d, type_offset)?,
            ty: u16_at(d, type_offset + 2)?,
            rows: u16_at(d, type_offset + 4)?,
            columns: u16_at(d, type_offset + 6)?,
            elements: u16_at(d, type_offset + 8)?,
            struct_members: u16_at(d, type_offset + 10)?,
            bytes: type_bytes(d, type_offset)?,
            default_offset: u32_at(d, c + 16)?,
        });
    }
    Some(Table { creator: string_at(d, creator_offset as usize)?, creator_offset, version, flags, constants, data })
}

/// D3DXGetShaderOutputSemantics: a vertex shader's output declarations
/// (usage, usage index) in declaration order.
pub fn output_semantics(s: &Shader) -> Vec<(u32, u32)> {
    s.instructions.iter()
        .filter(|i| i.opcode == op::DCL)
        .filter_map(|i| {
            let (d, t) = (i.dst.as_ref()?, i.decl?);
            (d.kind == reg::OUTPUT).then_some((t & 0x1F, (t >> 16) & 0xF))
        })
        .collect()
}
