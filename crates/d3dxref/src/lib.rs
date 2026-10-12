//! The host's d3dx9_43.dll (64-bit, part of the DirectX runtime) called
//! directly on shader bytecode: the oracle for the runtime's D3DX9_43
//! imports D3DXGetShaderConstantTableEx (with its ID3DXConstantTable) and
//! D3DXGetShaderOutputSemantics (brief L37). No D3D9 device is involved.
//! [`D3dx::load`] returns None where the DLL is missing; callers skip.
use std::ffi::{c_char, c_void, CStr};

type Obj = *mut c_void;

/// D3DXCONSTANT_DESC as the DLL fills it (pointers resolved).
#[derive(Clone, Debug, PartialEq)]
pub struct ConstantDesc {
    pub name: String,
    pub register_set: u32,
    pub register_index: u32,
    pub register_count: u32,
    pub class: u32,
    pub ty: u32,
    pub rows: u32,
    pub columns: u32,
    pub elements: u32,
    pub struct_members: u32,
    pub bytes: u32,
    /// the bytes DefaultValue points at (`bytes` of them), None for NULL
    pub default_value: Option<Vec<u8>>,
}

/// One top-level constant: GetConstant(NULL, i) and GetConstantDesc.
#[derive(Clone, Debug, PartialEq)]
pub struct Constant {
    /// GetConstantDesc with *pCount = 1: HRESULT, the count written back
    pub hr: i32,
    pub count: u32,
    pub desc: ConstantDesc,
    /// GetConstantDesc with *pCount = 0 (as the game calls it in its second
    /// loop): HRESULT, count written back, whether the description was
    /// written (the name pointer changed from the marker)
    pub count0: (i32, u32, bool),
}

/// What D3DXGetShaderConstantTableEx returns, through GetDesc and the
/// constants.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub creator: String,
    pub version: u32,
    pub constants: Vec<Constant>,
    /// GetConstant(NULL, Constants): the handle past the end is NULL
    pub past_end_is_null: bool,
}

type GetTable = unsafe extern "system" fn(*const u32, u32, *mut Obj) -> i32;
type GetSemantics = unsafe extern "system" fn(*const u32, *mut [u32; 2], *mut u32) -> i32;

extern "system" {
    fn LoadLibraryA(name: *const u8) -> Obj;
    fn GetProcAddress(module: Obj, name: *const u8) -> Obj;
}

pub struct D3dx {
    table: GetTable,
    semantics: GetSemantics,
}

unsafe fn method(o: Obj, slot: usize) -> *const c_void {
    *(*(o as *const *const *const c_void)).add(slot)
}

/// D3DXCONSTANT_DESC, x64 layout.
#[repr(C)]
struct RawDesc {
    name: *const c_char,
    register_set: u32,
    register_index: u32,
    register_count: u32,
    class: u32,
    ty: u32,
    rows: u32,
    columns: u32,
    elements: u32,
    struct_members: u32,
    bytes: u32,
    default_value: *const u8,
}

impl D3dx {
    pub fn load() -> Option<D3dx> {
        unsafe {
            let module = LoadLibraryA(c"d3dx9_43.dll".as_ptr() as *const u8);
            if module.is_null() {
                return None;
            }
            let t = GetProcAddress(module, c"D3DXGetShaderConstantTableEx".as_ptr() as *const u8);
            let s = GetProcAddress(module, c"D3DXGetShaderOutputSemantics".as_ptr() as *const u8);
            if t.is_null() || s.is_null() {
                return None;
            }
            Some(D3dx { table: std::mem::transmute::<Obj, GetTable>(t), semantics: std::mem::transmute::<Obj, GetSemantics>(s) })
        }
    }

    /// D3DXGetShaderConstantTableEx(tokens, flags) read through GetDesc
    /// (slot 5), GetConstant (slot 8) and GetConstantDesc (slot 6), as the
    /// game reads it; Err(HRESULT) when the call fails.
    pub fn constant_table(&self, tokens: &[u32], flags: u32) -> Result<Table, i32> {
        unsafe {
            let mut t: Obj = std::ptr::null_mut();
            let hr = (self.table)(tokens.as_ptr(), flags, &mut t);
            if hr < 0 || t.is_null() {
                return Err(hr);
            }
            // D3DXCONSTANTTABLE_DESC: Creator, Version, Constants
            #[repr(C)]
            struct Desc {
                creator: *const c_char,
                version: u32,
                constants: u32,
            }
            let mut d = Desc { creator: std::ptr::null(), version: 0, constants: 0 };
            let get_desc: unsafe extern "system" fn(Obj, *mut Desc) -> i32 = std::mem::transmute(method(t, 5));
            get_desc(t, &mut d);
            let get_constant: unsafe extern "system" fn(Obj, *const c_char, u32) -> *const c_char = std::mem::transmute(method(t, 8));
            let get_constant_desc: unsafe extern "system" fn(Obj, *const c_char, *mut RawDesc, *mut u32) -> i32 = std::mem::transmute(method(t, 6));
            let string = |p: *const c_char| if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() };
            let mut constants = Vec::new();
            for k in 0..d.constants {
                let h = get_constant(t, std::ptr::null(), k);
                let mut raw: RawDesc = std::mem::zeroed();
                let mut count = 1u32;
                let hr = get_constant_desc(t, h, &mut raw, &mut count);
                let desc = ConstantDesc {
                    name: string(raw.name),
                    register_set: raw.register_set,
                    register_index: raw.register_index,
                    register_count: raw.register_count,
                    class: raw.class,
                    ty: raw.ty,
                    rows: raw.rows,
                    columns: raw.columns,
                    elements: raw.elements,
                    struct_members: raw.struct_members,
                    bytes: raw.bytes,
                    default_value: (!raw.default_value.is_null()).then(|| std::slice::from_raw_parts(raw.default_value, raw.bytes as usize).to_vec()),
                };
                let marker = c"marker".as_ptr();
                let mut raw0: RawDesc = std::mem::zeroed();
                raw0.name = marker;
                let mut count0 = 0u32;
                let hr0 = get_constant_desc(t, h, &mut raw0, &mut count0);
                constants.push(Constant { hr, count, desc, count0: (hr0, count0, raw0.name != marker) });
            }
            let past_end_is_null = get_constant(t, std::ptr::null(), d.constants).is_null();
            let table = Table { creator: string(d.creator), version: d.version, constants, past_end_is_null };
            let release: unsafe extern "system" fn(Obj) -> u32 = std::mem::transmute(method(t, 2));
            release(t);
            Ok(table)
        }
    }

    /// D3DXGetShaderOutputSemantics(tokens): (usage, usage index) pairs and
    /// the count with a NULL array; Err(HRESULT) when the call fails.
    pub fn output_semantics(&self, tokens: &[u32]) -> Result<(Vec<(u32, u32)>, u32), i32> {
        unsafe {
            let mut out = [[0u32; 2]; 64];
            let mut count = 0u32;
            let hr = (self.semantics)(tokens.as_ptr(), out.as_mut_ptr(), &mut count);
            if hr < 0 {
                return Err(hr);
            }
            let mut count_only = 0u32;
            (self.semantics)(tokens.as_ptr(), std::ptr::null_mut(), &mut count_only);
            Ok((out[..count as usize].iter().map(|s| (s[0], s[1])).collect(), count_only))
        }
    }
}
