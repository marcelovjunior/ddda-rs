//! The readable ports behind the lifted program's calling convention
//! (docs/CORE.md, "Wiring"; the same contract as `mtmath::abi`).
//!
//! Every `f_XXXXXXXX` has a lifted function's signature and replaces the
//! lifted body at that address through `research/native/lift.py gen`. It
//! leaves the machine as the original's `ret` does: all eight general
//! registers, xmm0, the x87 state, and every byte the original writes,
//! including its pushes below ESP. Calls the original makes are made the
//! way the lifted body makes them, with the original's return addresses
//! and argument pushes: virtual calls, imports, and calls to functions
//! that themselves call out (the recursive DTI searches, MtArray::clear,
//! MtPropertyList::clear) through `x86::call_ext` (the runtime binds the
//! ported ones to their ports); the platform models (node pool, memset,
//! memcpy) and the DTI allocator lookup directly.

use crate::allocator::{self, Registered};
use crate::array::{self, Array};
use crate::dti::{self, Dti, NameCompare};
use crate::hash;
use crate::machine::{arg, call, call_out, pop, push, ret};
use crate::object;
use crate::property;
use ddnative_base::mem::Mem;
use ddnative_base::platform;
use ddnative_base::x86::{self, Cpu, LiftFn};
use std::cell::RefCell;

/// The import slots of EnterCriticalSection and LeaveCriticalSection.
const ENTER_CRITICAL_SECTION: u32 = 0x0139_D160;
const LEAVE_CRITICAL_SECTION: u32 = 0x0139_D164;

/// cdecl `MtAllocator *allocator(const MtDTI *)`; leaves the index in ECX.
pub fn f_00cf5e20(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (a, index) = dti::allocator_of(m, Dti(arg(m, cpu, 0)));
    (cpu.eax, cpu.ecx) = (a, index);
    ret(cpu, 0);
}

/// cdecl `MtDTI *find(u32 id)`; ECX = the id.
pub fn f_00cf55b0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let id = arg(m, cpu, 0);
    cpu.ecx = id;
    cpu.eax = dti::find_by_id(m, id).map_or(0, |d| d.0);
    ret(cpu, 0);
}

/// cdecl `u32 crc(const char *, u32)`: EDX ends at the terminator; a
/// non-empty string pushes ESI and leaves ECX = 0, an empty one only
/// clears CL.
pub fn f_00d01b60(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (s, crc) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let (value, end) = hash::crc_str(m, s, crc);
    if end == s {
        cpu.ecx &= !0xFF;
    } else {
        m.set_u32(cpu.esp.wrapping_sub(4), cpu.esi);
        cpu.ecx = 0;
    }
    (cpu.eax, cpu.edx) = (value, end);
    ret(cpu, 0);
}

/// thiscall `MtObject::~MtObject()` body.
pub fn f_004038f0(m: &mut dyn Mem, cpu: &mut Cpu) {
    object::destroy(m, cpu.ecx);
    ret(cpu, 0);
}

/// cdecl `void *operator new(size)`: the allocator's virtual allocate
/// (thiscall, `ret 8`) with ECX = the allocator, EAX = the function, EDX =
/// the size.
pub fn f_00cf5c00(m: &mut dyn Mem, cpu: &mut Cpu) {
    let size = arg(m, cpu, 0);
    object::new(m, size, |m, allocator, size, align| {
        cpu.ecx = allocator;
        cpu.eax = m.u32(m.u32(allocator).wrapping_add(object::ALLOC_SLOT));
        cpu.edx = size;
        push(m, cpu, align);
        push(m, cpu, size);
        call_out(m, cpu, cpu.eax, 0x00CF_5C14);
        cpu.eax
    });
    ret(cpu, 0);
}

/// thiscall `MtPropertyList::MtPropertyList()`, returns `this`.
pub fn f_00cf7340(m: &mut dyn Mem, cpu: &mut Cpu) {
    property::construct(m, cpu.ecx);
    cpu.eax = cpu.ecx;
    ret(cpu, 0);
}

/// thiscall `MtProperty *MtPropertyList::add(const MtProperty *)`, `ret 4`.
/// Pushes ESI, takes a node from the pool model; leaves ECX = the old head,
/// EDX = 0 and the property's last quadword in xmm0.
pub fn f_004027a0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (this, prop) = (cpu.ecx, arg(m, cpu, 0));
    let esi = cpu.esi;
    push(m, cpu, esi);
    cpu.esi = this;
    let node = property::add(m, this, prop, |m| {
        call(m, cpu, platform::pool40_alloc, 0x0040_27A8);
        cpu.eax
    });
    cpu.xmm[0] = [
        m.u32(prop.wrapping_add(0x20)),
        m.u32(prop.wrapping_add(0x24)),
        0,
        0,
    ];
    (cpu.eax, cpu.edx) = (node, 0);
    cpu.ecx = m.u32(node.wrapping_add(0x24)); // the old head (or 0)
    cpu.esi = pop(m, cpu);
    ret(cpu, 4);
}

/// thiscall `MtPropertyList::clear()`: pushes ESI, EDI and the pool lock;
/// EnterCriticalSection and LeaveCriticalSection through their imports.
pub fn f_00cf74e0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (esi, edi, this) = (cpu.esi, cpu.edi, cpu.ecx);
    push(m, cpu, esi);
    push(m, cpu, edi);
    push(m, cpu, property::POOL_LOCK);
    cpu.edi = this;
    let enter = m.u32(ENTER_CRITICAL_SECTION);
    call_out(m, cpu, enter, 0x00CF_74EF);
    let last = property::return_nodes(m, this);
    if let Some(node) = last {
        (cpu.ecx, cpu.edx) = (node, 0);
    }
    (cpu.eax, cpu.esi) = (0, 0);
    push(m, cpu, property::POOL_LOCK);
    let leave = m.u32(LEAVE_CRITICAL_SECTION);
    call_out(m, cpu, leave, 0x00CF_7537);
    cpu.edi = pop(m, cpu);
    cpu.esi = pop(m, cpu);
    ret(cpu, 0);
}

/// thiscall `MtPropertyList::~MtPropertyList()` body: pushes ESI, calls
/// the clear port.
pub fn f_00cf76c0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (esi, this) = (cpu.esi, cpu.ecx);
    push(m, cpu, esi);
    cpu.esi = this;
    property::destroy(m, this, |m, _| call_out(m, cpu, 0x00CF_74E0, 0x00CF_76CE));
    cpu.esi = pop(m, cpu);
    ret(cpu, 0);
}

/// The array allocator's free, as the MtArray functions call it: the DTI
/// allocator port with MtArray's DTI pushed (cdecl, then `add esp, 4`),
/// then the allocator's virtual free (thiscall, `ret 4`) with the block
/// pushed (the original re-reads the array's storage pointer for it).
/// `back` are the two return addresses; `cleanup` the caller's `add esp,
/// N` after the DTI call.
fn free_array_block(m: &mut dyn Mem, cpu: &mut Cpu, this: u32, back: [u32; 2], cleanup: u32) {
    push(m, cpu, array::DTI);
    call(m, cpu, f_00cf5e20, back[0]);
    let data = Array(this).data(m);
    cpu.ecx = data;
    cpu.edx = m.u32(m.u32(cpu.eax).wrapping_add(object::FREE_SLOT));
    cpu.esp = cpu.esp.wrapping_add(cleanup);
    push(m, cpu, data);
    cpu.ecx = cpu.eax;
    call_out(m, cpu, cpu.edx, back[1]);
}

/// thiscall `MtArray::clear(bool free_storage)`, `ret 4`: pushes EBX,
/// ESI, EDI; each element's deleting destructor through its vtable.
pub fn f_004025c0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let this = cpu.ecx;
    let free_storage = m.u8(cpu.esp.wrapping_add(4)) != 0;
    let saved = [cpu.ebx, cpu.esi, cpu.edi];
    for r in saved {
        push(m, cpu, r);
    }
    (cpu.esi, cpu.ebx, cpu.edi) = (this, 0, 0);
    let a = Array(this);
    let cell = RefCell::new(cpu);
    array::clear(
        m,
        a,
        free_storage,
        |m, index, element| {
            let mut g = cell.borrow_mut();
            let cpu: &mut Cpu = &mut g;
            cpu.edi = index;
            cpu.eax = a.data(m);
            match element {
                Some(e) => {
                    cpu.ecx = e;
                    cpu.edx = m.u32(e);
                    cpu.eax = m.u32(cpu.edx);
                    push(m, cpu, 1);
                    call_out(m, cpu, cpu.eax, 0x0040_25E2);
                }
                None => cpu.ecx = 0,
            }
            cpu.edi = index + 1;
        },
        |m, _| free_array_block(m, &mut cell.borrow_mut(), this, [0x0040_25FD, 0x0040_260D], 4),
    );
    let cpu = cell.into_inner();
    cpu.edi = pop(m, cpu);
    cpu.esi = pop(m, cpu);
    cpu.ebx = pop(m, cpu);
    ret(cpu, 4);
}

/// thiscall `MtArray::reserve(u32 capacity)`, `ret 4`: pushes EBX, EBP,
/// ESI, EDI; the DTI allocator port twice, the allocator's virtual
/// allocate and free, the memset and memcpy models.
pub fn f_00402620(m: &mut dyn Mem, cpu: &mut Cpu) {
    let this = cpu.ecx;
    let capacity = arg(m, cpu, 0);
    let saved = [cpu.ebx, cpu.ebp, cpu.esi, cpu.edi];
    for r in saved {
        push(m, cpu, r);
    }
    cpu.esi = this;
    let cell = RefCell::new(cpu);
    array::reserve(
        m,
        Array(this),
        capacity,
        |m, size, align| {
            let mut g = cell.borrow_mut();
            let cpu: &mut Cpu = &mut g;
            push(m, cpu, array::DTI);
            call(m, cpu, f_00cf5e20, 0x0040_2630);
            cpu.ebp = capacity;
            cpu.edx = m.u32(cpu.eax);
            cpu.esp = cpu.esp.wrapping_add(4);
            push(m, cpu, align);
            cpu.ebx = size;
            cpu.ecx = cpu.eax;
            cpu.eax = m.u32(cpu.edx.wrapping_add(object::ALLOC_SLOT));
            push(m, cpu, size);
            call_out(m, cpu, cpu.eax, 0x0040_264A);
            cpu.edi = cpu.eax;
            cpu.eax
        },
        |m, block, value, size| {
            let mut g = cell.borrow_mut();
            let cpu: &mut Cpu = &mut g;
            push(m, cpu, size);
            push(m, cpu, value);
            push(m, cpu, block);
            call(m, cpu, platform::memset, 0x0040_2655);
        },
        |m, block, data, size| {
            let mut g = cell.borrow_mut();
            let cpu: &mut Cpu = &mut g;
            cpu.ecx = size;
            cpu.edx = data;
            push(m, cpu, size);
            push(m, cpu, data);
            push(m, cpu, block);
            call(m, cpu, platform::memcpy, 0x0040_2667);
        },
        |m, _| free_array_block(m, &mut cell.borrow_mut(), this, [0x0040_2671, 0x0040_2681], 0x1C),
    );
    let cpu = cell.into_inner();
    cpu.edi = pop(m, cpu);
    cpu.esi = pop(m, cpu);
    cpu.ebp = pop(m, cpu);
    cpu.ebx = pop(m, cpu);
    ret(cpu, 4);
}

/// thiscall `MtArray::~MtArray()` body: pushes ESI; the clear port when
/// the array owns its elements; the array allocator's free.
pub fn f_00402530(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (esi, this) = (cpu.esi, cpu.ecx);
    push(m, cpu, esi);
    cpu.esi = this;
    let cell = RefCell::new(cpu);
    array::destroy(
        m,
        Array(this),
        |m, _| {
            let mut g = cell.borrow_mut();
            let cpu: &mut Cpu = &mut g;
            push(m, cpu, 1);
            call_out(m, cpu, 0x0040_25C0, 0x0040_2546);
        },
        |m, _| free_array_block(m, &mut cell.borrow_mut(), this, [0x0040_2556, 0x0040_2566], 4),
    );
    let cpu = cell.into_inner();
    cpu.esi = pop(m, cpu);
    ret(cpu, 0);
}

/// cdecl `allocator registry add(MtAllocator *)`.
pub fn f_00cf5450(m: &mut dyn Mem, cpu: &mut Cpu) {
    let a = arg(m, cpu, 0);
    (cpu.eax, cpu.ecx) = match allocator::register(m, a) {
        Registered::Reused(i) => (i, a),
        Registered::Appended(i) => (a, i.wrapping_add(1)),
    };
    ret(cpu, 0);
}

/// cdecl `allocator registry remove(MtAllocator *)`: EDX = the allocator
/// once the table has more than one slot.
pub fn f_00cf54a0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let len = m.u32(allocator::REGISTRY_LEN);
    // the original reads its argument only when there is a slot to search
    let a = if len > 1 { arg(m, cpu, 0) } else { 0 };
    cpu.eax = allocator::unregister(m, a);
    cpu.ecx = len;
    if len > 1 {
        cpu.edx = a;
    }
    ret(cpu, 0);
}

/// cdecl `MtDTI::setAllocator(MtDTI *, u32 index)`: tail-jumps into the
/// propagation port with ECX = the DTI and EAX = the bits it flipped.
pub fn f_00cf5e40(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (d, index) = (Dti(arg(m, cpu, 0)), arg(m, cpu, 1));
    let changed = ((index << dti::ALLOCATOR_SHIFT) ^ d.size_flags(m)) & dti::ALLOCATOR_MASK;
    dti::set_allocator(m, d, index, |m, d| {
        (cpu.eax, cpu.ecx) = (changed, d.0);
        x86::call_ext(m, cpu, 0x00CF_5750); // jmp: returns to our caller
    });
}

/// thiscall `MtDTI::propagateAllocator()`: pushes ESI and EDI; recurses
/// through its own adapter.
pub fn f_00cf5750(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (esi, edi, this) = (cpu.esi, cpu.edi, cpu.ecx);
    push(m, cpu, esi);
    push(m, cpu, edi);
    cpu.edi = this;
    dti::propagate_allocator(m, Dti(this), |m, child, changed| {
        (cpu.esi, cpu.eax, cpu.ecx) = (child.0, changed, child.0);
        call_out(m, cpu, 0x00CF_5750, 0x00CF_5775);
    });
    cpu.edi = pop(m, cpu);
    cpu.esi = pop(m, cpu);
    ret(cpu, 0);
}

/// The registers a name comparison leaves in the child search (EAX = the
/// order, ECX at the end of the query, DL = the last byte).
fn compared(cpu: &mut Cpu, c: NameCompare) {
    cpu.eax = c.order as u32;
    cpu.ecx = c.b_end;
    cpu.edx = (cpu.edx & !0xFF) | c.last as u32;
}

/// thiscall `MtDTI *MtDTI::findChild(const char *)`, `ret 4`: pushes ESI
/// and EDI; recurses through its own adapter.
pub fn f_00cf55e0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (esi, edi, this) = (cpu.esi, cpu.edi, cpu.ecx);
    push(m, cpu, esi);
    let parent = Dti(this);
    push(m, cpu, edi);
    let found = match parent.child(m) {
        None => None,
        Some(_) => {
            let name = m.u32(cpu.esp.wrapping_add(0xC));
            cpu.edi = name;
            let cell = RefCell::new(&mut *cpu);
            dti::find_child_by_name(
                m,
                parent,
                name,
                |m, child| {
                    let mut g = cell.borrow_mut();
                    let cpu: &mut Cpu = &mut g;
                    (cpu.esi, cpu.ecx) = (child.0, child.0);
                    push(m, cpu, name);
                    call_out(m, cpu, 0x00CF_55E0, 0x00CF_5626);
                    (cpu.eax != 0).then_some(Dti(cpu.eax))
                },
                |c| compared(&mut cell.borrow_mut(), c),
            )
        }
    };
    cpu.eax = found.map_or(0, |d| d.0);
    cpu.edi = pop(m, cpu);
    cpu.esi = pop(m, cpu);
    ret(cpu, 4);
}

/// cdecl `MtDTI *find(const char *name, MtDTI *root)`: pushes EDI (and
/// EBX, ESI around the root's name comparison); the child search port.
pub fn f_00cf5870(m: &mut dyn Mem, cpu: &mut Cpu) {
    let (name, root) = (arg(m, cpu, 0), arg(m, cpu, 1));
    let edi = cpu.edi;
    push(m, cpu, edi);
    cpu.edi = name;
    if name != 0 {
        cpu.eax = root;
        cpu.edx = Dti(root).name(m);
        if cpu.edx != 0 {
            // push ebx; push esi (popped again after the comparison)
            m.set_u32(cpu.esp.wrapping_sub(4), cpu.ebx);
            m.set_u32(cpu.esp.wrapping_sub(8), cpu.esi);
        }
    }
    let found = {
        let cell = RefCell::new(&mut *cpu);
        dti::find_by_name(
            m,
            name,
            Dti(root),
            |m, root| {
                let mut g = cell.borrow_mut();
                let cpu: &mut Cpu = &mut g;
                cpu.ecx = root.0;
                push(m, cpu, name);
                call_out(m, cpu, 0x00CF_55E0, 0x00CF_58C3);
                (cpu.eax != 0).then_some(Dti(cpu.eax))
            },
            |c| cell.borrow_mut().edx = c.order as u32,
        )
    };
    cpu.eax = found.map_or(0, |d| d.0);
    cpu.edi = pop(m, cpu);
    ret(cpu, 0);
}

/// thiscall `void equal_range(pair<node*, node*> *out, const u32 *key)`
/// of a `std::set<u32>` (ECX = the tree), `ret 8`: EAX = out, EDX = the
/// upper bound, ECX = the key when the upper-bound walk ran; EBX, EBP and,
/// when the tree is not empty, ESI and EDI pushed below ESP and restored.
/// Called on every tracked object's destruction (the QLOC alive set):
/// ~14% of the render thread in gameplay as lifted code at opt-level 0.
pub fn f_00db82c0(m: &mut dyn Mem, cpu: &mut Cpu) {
    let esp = cpu.esp;
    let root = m.u32(m.u32(cpu.ecx.wrapping_add(4)).wrapping_add(4));
    m.set_u32(esp.wrapping_sub(4), cpu.ebx);
    m.set_u32(esp.wrapping_sub(8), cpu.ebp);
    if m.u8(root.wrapping_add(0x11)) == 0 {
        m.set_u32(esp.wrapping_sub(12), cpu.esi);
        m.set_u32(esp.wrapping_sub(16), cpu.edi);
    }
    let r = crate::tree::equal_range(m, cpu.ecx, arg(m, cpu, 1));
    let out = arg(m, cpu, 0);
    m.set_u32(out, r.lower);
    m.set_u32(out.wrapping_add(4), r.upper);
    if r.second_walk {
        cpu.ecx = r.key;
    }
    (cpu.eax, cpu.edx) = (out, r.upper);
    ret(cpu, 8);
}

/// Every port above with its original address and the xmm registers it
/// leaves exactly as the original does (bit k = xmmk; these use no xmm
/// scratch except `add`, which sets xmm0 as the original does).
pub const PORTS: &[(u32, LiftFn, u8)] = &[
    (0x0040_2530, f_00402530, 0b1111_1111),
    (0x0040_25C0, f_004025c0, 0b1111_1111),
    (0x0040_2620, f_00402620, 0b1111_1111),
    (0x0040_27A0, f_004027a0, 0b1111_1111),
    (0x0040_38F0, f_004038f0, 0b1111_1111),
    (0x00CF_5450, f_00cf5450, 0b1111_1111),
    (0x00CF_54A0, f_00cf54a0, 0b1111_1111),
    (0x00CF_55B0, f_00cf55b0, 0b1111_1111),
    (0x00CF_55E0, f_00cf55e0, 0b1111_1111),
    (0x00CF_5750, f_00cf5750, 0b1111_1111),
    (0x00CF_5870, f_00cf5870, 0b1111_1111),
    (0x00CF_5C00, f_00cf5c00, 0b1111_1111),
    (0x00CF_5E20, f_00cf5e20, 0b1111_1111),
    (0x00CF_5E40, f_00cf5e40, 0b1111_1111),
    (0x00CF_7340, f_00cf7340, 0b1111_1111),
    (0x00CF_74E0, f_00cf74e0, 0b1111_1111),
    (0x00CF_76C0, f_00cf76c0, 0b1111_1111),
    (0x00D0_1B60, f_00d01b60, 0b1111_1111),
    (0x00DB_82C0, f_00db82c0, 0b1111_1111),
];
