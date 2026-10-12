//! The D3DX9_43 imports the game calls when it links a shader pair
//! (0x00E10690, from sShader2 through 0x00E12850): D3DXGetShaderConstantTableEx
//! (flags 0x20000, call at 0x00E106BF) with the ID3DXConstantTable it
//! returns, and D3DXGetShaderOutputSemantics (0x00E10F6A). Both read only
//! the shader bytecode; the answers come from `d3d9shader::ctab`, which
//! equals d3dx9_43.dll on all 29,198 game shaders (crates/d3dxref).
//!
//! The table object keeps a guest copy of the CTAB block: names, the
//! creator string and default values point into it, as the DLL's pointers
//! point into its own copy. A constant's handle is the guest address of its
//! D3DXSHADER_CONSTANTINFO record in that copy. The game uses GetDesc,
//! GetConstant(NULL, i), GetConstantDesc and Release; the other methods
//! stop with their name.
use crate::d3d9::{D3DERR_INVALIDCALL, D3D_OK};
use crate::d3d9dev::{add_ref_to, new_object, objects, release_of, Kind};
use crate::interfaces::Spec;
use crate::services::{arg, ret, with_state};
use ddnative_base::mem::Mem;
use ddnative_base::x86::Cpu;

/// D3DXERR_INVALIDDATA: what d3dx9_43.dll answers for a shader without a
/// constant table (measured, d3dxref edge_cases).
pub const D3DXERR_INVALIDDATA: u32 = 0x8876_0B59;
/// D3DXCONSTTABLE_LARGEADDRESSAWARE, the only flags the game passes.
const LARGEADDRESSAWARE: u32 = 0x20000;

/// A constant table object's data (Kind::ConstantTable).
#[derive(Debug)]
pub struct Table {
    /// guest copy of the CTAB block
    pub buffer: u32,
    pub reflection: d3d9shader::ctab::Table,
    /// per top-level constant: its handle
    pub handles: Vec<u32>,
}

impl Table {
    pub fn free(&self) {
        with_state(|s| s.heap.free(self.buffer)).expect("constant table buffer");
    }
}

/// A shader's tokens in guest memory, up to and including END.
fn read_tokens(m: &dyn Mem, p: u32) -> Vec<u32> {
    let mut t = vec![m.u32(p)];
    let mut at = 1u32;
    loop {
        let tok = m.u32(p + 4 * at);
        t.push(tok);
        at += 1;
        if tok & 0xFFFF == 0xFFFF {
            break;
        }
        let len = if tok & 0xFFFF == 0xFFFE { (tok >> 16) & 0x7FFF } else { (tok >> 24) & 0xF };
        for _ in 0..len {
            t.push(m.u32(p + 4 * at));
            at += 1;
        }
    }
    t
}

/// D3DXGetShaderConstantTableEx(function, flags, ID3DXConstantTable**):
/// D3DXERR_INVALIDDATA for a shader without a constant table.
fn get_shader_constant_table_ex(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (function, flags, out) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    assert_eq!(flags, LARGEADDRESSAWARE, "D3DXGetShaderConstantTableEx flags {flags:#x}: only D3DXCONSTTABLE_LARGEADDRESSAWARE is modelled");
    let tokens = read_tokens(m, function);
    let reflection = d3d9shader::parse(&tokens).ok().and_then(|s| d3d9shader::ctab::table(&s));
    cpu.eax = match reflection {
        None => D3DXERR_INVALIDDATA,
        Some(reflection) => {
            let size = reflection.data.len() as u32;
            let buffer = with_state(|s| s.heap.alloc(size.max(4))).expect("guest heap");
            for (k, &b) in reflection.data.iter().enumerate() {
                m.set_u8(buffer + k as u32, b);
            }
            // D3DXSHADER_CONSTANTTABLE.ConstantInfo: the records' offset
            let info = u32::from_le_bytes(reflection.data[16..20].try_into().unwrap());
            let handles = (0..reflection.constants.len() as u32).map(|k| buffer + info + 20 * k).collect();
            let table = new_object(m, "ID3DXConstantTable", Kind::ConstantTable { data: Box::new(Table { buffer, reflection, handles }) });
            m.set_u32(out, table);
            D3D_OK
        }
    };
    ret(m, cpu, 12);
}

/// D3DXGetShaderOutputSemantics(function, D3DXSEMANTIC*, UINT* count): the
/// output declarations (usage, usage index); a NULL array asks the count.
fn get_shader_output_semantics(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (function, out, count) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    let tokens = read_tokens(m, function);
    cpu.eax = match d3d9shader::parse(&tokens) {
        Ok(s) => {
            let semantics = d3d9shader::ctab::output_semantics(&s);
            if out != 0 {
                for (k, &(usage, index)) in semantics.iter().enumerate() {
                    m.set_u32(out + 8 * k as u32, usage);
                    m.set_u32(out + 8 * k as u32 + 4, index);
                }
            }
            if count != 0 {
                m.set_u32(count, semantics.len() as u32);
            }
            D3D_OK
        }
        Err(_) => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 12);
}

fn with_table<R>(this: u32, f: impl FnOnce(&Table) -> R) -> R {
    objects(|o| match o.live.get(&this).map(|x| &x.kind) {
        Some(Kind::ConstantTable { data }) => f(data),
        _ => panic!("{this:#x} is not a live ID3DXConstantTable"),
    })
}

fn add_ref(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = add_ref_to(arg(m, cpu, 0));
    ret(m, cpu, 4);
}
fn release(m: &mut dyn Mem, cpu: &mut Cpu) {
    cpu.eax = release_of(arg(m, cpu, 0));
    ret(m, cpu, 4);
}

/// GetDesc(this, D3DXCONSTANTTABLE_DESC*): Creator, Version, Constants.
fn get_desc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, out) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let (creator, version, count) = with_table(this, |t| (t.buffer + t.reflection.creator_offset, t.reflection.version, t.reflection.constants.len() as u32));
    for (k, v) in [creator, version, count].into_iter().enumerate() {
        m.set_u32(out + 4 * k as u32, v);
    }
    cpu.eax = D3D_OK;
    ret(m, cpu, 8);
}

/// GetConstant(this, parent handle, index): a top-level constant's handle
/// (NULL past the end, as the DLL answers). Members of structures (a
/// parent handle) are not modelled: the game asks only top-level ones.
fn get_constant(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, parent, index) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2));
    assert_eq!(parent, 0, "ID3DXConstantTable::GetConstant with a parent handle is not modelled");
    cpu.eax = with_table(this, |t| t.handles.get(index as usize).copied().unwrap_or(0));
    ret(m, cpu, 12);
}

/// GetConstantDesc(this, handle, D3DXCONSTANT_DESC*, UINT* count): one
/// description; the DLL writes it and a count of 1 even when *count is 0
/// (measured on every game shader's constants).
fn get_constant_desc(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, handle, out, count) = (arg(m, cpu, 0), arg(m, cpu, 1), arg(m, cpu, 2), arg(m, cpu, 3));
    let desc = with_table(this, |t| {
        let k = t.handles.iter().position(|&h| h == handle)?;
        let c = &t.reflection.constants[k];
        let default = if c.default_offset != 0 { t.buffer + c.default_offset } else { 0 };
        Some([t.buffer + c.name_offset, c.register_set, c.register_index, c.register_count, c.class, c.ty, c.rows, c.columns,
              c.elements, c.struct_members, c.bytes, default])
    });
    cpu.eax = match desc {
        Some(d) => {
            if out != 0 {
                for (k, v) in d.into_iter().enumerate() {
                    m.set_u32(out + 4 * k as u32, v);
                }
            }
            if count != 0 {
                m.set_u32(count, 1);
            }
            D3D_OK
        }
        None => D3DERR_INVALIDCALL,
    };
    ret(m, cpu, 16);
}

pub const APIS: &[(&str, &str, fn(&mut dyn Mem, &mut Cpu))] = &[
    ("d3dx9_43.dll", "D3DXGetShaderConstantTableEx", get_shader_constant_table_ex),
    ("d3dx9_43.dll", "D3DXGetShaderOutputSemantics", get_shader_output_semantics),
];

pub const INTERFACES: &[Spec] = &[Spec {
    name: "ID3DXConstantTable",
    slots: 27,
    methods: &[(1, add_ref), (2, release), (5, get_desc), (6, get_constant_desc), (8, get_constant)],
    names: &[
        "QueryInterface", "AddRef", "Release", "GetBufferPointer", "GetBufferSize", "GetDesc", "GetConstantDesc",
        "GetSamplerIndex", "GetConstant", "GetConstantByName", "GetConstantElement", "SetDefaults", "SetValue", "SetBool",
        "SetBoolArray", "SetInt", "SetIntArray", "SetFloat", "SetFloatArray", "SetVector", "SetVectorArray", "SetMatrix",
        "SetMatrixArray", "SetMatrixPointerArray", "SetMatrixTranspose", "SetMatrixTransposeArray", "SetMatrixTransposePointerArray",
    ],
}];
