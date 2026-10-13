//! The MT Framework core library of DDDA.exe as readable Rust
//! (docs/CORE.md): run-time type descriptors (MtDTI), the string hash,
//! MtObject, MtArray, MtPropertyList and the allocator registry.
//!
//! Each function cites the address it translates and is verified against
//! the original by differential tests (`crates/mtcore/tests/
//! differential.rs`): the lifted program's own vectors plus edge cases
//! recorded from the original (`research/native/vectors_core.py`). The
//! layouts are the original's (`#[repr(C)]`); guest objects are addressed
//! through small handles (`dti::Dti`, `array::Array`) over guest memory.
//!
//! - [`dti`]: descriptors: allocator lookup, find by id and by name,
//!   allocator propagation.
//! - [`hash`]: the CRC-32 string hash behind DTI ids.
//! - [`object`]: MtObject's destructor body and `operator new`.
//! - [`array`]: MtArray clear, reserve, destructor.
//! - [`property`]: MtPropertyList construction, `add`, clear, destructor.
//! - [`allocator`]: the allocator registry.
//! - [`tree`]: MSVC's red-black tree (`equal_range` of the QLOC alive set).
//! - [`abi`]: the ports behind the lifted calling convention; the lifted
//!   program calls these instead of the lifted bodies.

pub mod abi;
pub mod allocator;
pub mod array;
pub mod dti;
pub mod hash;
pub mod hot;
pub mod job_hash;
pub mod job_lookup;
pub mod job_shadow;
pub mod machine;
pub mod semantic;
pub mod semantic_s1;
pub mod semantic_jobs;
pub mod semantic_s4;
pub mod semantic_s4_model;
pub mod semantic_s4_player;
pub mod semantic_instancing;
pub mod semantic_draw;
pub mod semantic_s9;
pub mod semantic_draw_c;
pub mod semantic_draw_b;
pub mod semantic_draw_a;
pub mod object;
pub mod property;
pub mod shadow_traversal;
pub mod shadow_pass;
pub mod shadow_lanes;
pub mod semantic_c2;
pub mod semantic_s2;
pub mod semantic_s3;
pub mod semantic_s5;
pub mod semantic_s6;
pub mod semantic_s6_aim;
pub mod semantic_s6_pose;
pub mod semantic_s7;
pub mod semantic_s7_trig;
pub mod semantic_s7_math;
pub mod semantic_s8;
pub mod tree;
