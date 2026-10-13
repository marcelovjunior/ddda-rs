//! Stable unsigned pair-key merge sort used by worker jobs.
use ddnative_base::{mem::{Mem, Pages}, x86::{self, Cpu}};

fn push(m: &mut dyn Mem, c: &mut Cpu, p: Pages, value: u32) {
    c.esp = c.esp.wrapping_sub(4);
    p.set_u32(m, c.esp, value);
}
fn pop(m: &dyn Mem, c: &mut Cpu, p: Pages) -> u32 {
    let value = p.u32(m, c.esp);
    c.esp = c.esp.wrapping_add(4);
    value
}

// Translated from 0x00FEDD60 (DDDA.exe 2017-12-13 Steam).
/// Alternate the caller's two buffers, retaining original stack scratch,
/// ordered word copies (including alias effects), and original memcpy calls.
pub fn sort_pairs(m: &mut dyn Mem, c: &mut Cpu) {
    let p = Pages::of(m);
    macro_rules! s { ($offset:expr) => {p.u32(m,c.esp.wrapping_add($offset))}; }
    macro_rules! w { ($offset:expr,$value:expr) => {{let value=$value;p.set_u32(m,c.esp.wrapping_add($offset),value)}}; }
    macro_rules! r { ($base:expr,$offset:expr) => {p.u32(m,$base.wrapping_add($offset))}; }
    macro_rules! put { ($base:expr,$offset:expr,$value:expr) => {p.set_u32(m,$base.wrapping_add($offset),$value)}; }
    c.esp=c.esp.wrapping_sub(0x30);
    push(m,c,p,c.ebx);push(m,c,p,c.ebp);push(m,c,p,c.esi);push(m,c,p,c.edi);
    c.ebp=1;c.eax=0;
    loop {
        c.edi=s!(0x48);
        if (c.edi as i32)<=(c.ebp as i32){break;}
        c.eax=c.eax.wrapping_add(1);c.ebp=c.ebp.wrapping_add(c.ebp);
        if (c.eax as i32)>=32{break;}
    }
    w!(0x1c,c.ebp);
    let mut use_input=(c.ebp as i32)<2;
    if !use_input {
        c.ebx=s!(0x4c);c.edx=0;c.eax=c.ebx;
        if (c.edi as i32)>0 {
            c.ecx=s!(0x44);
            loop {
                c.esi=c.edx.wrapping_add(1);
                let single=(c.esi as i32)>=(c.edi as i32);
                c.esi=r!(c.ecx,0);
                if single {
                    put!(c.eax,0,c.esi);c.esi=r!(c.ecx,4);put!(c.eax,4,c.esi);
                    c.eax=c.eax.wrapping_add(8);
                } else {
                    if c.esi>r!(c.ecx,8) {
                        c.esi=r!(c.ecx,8);put!(c.eax,0,c.esi);
                        c.esi=r!(c.ecx,12);put!(c.eax,4,c.esi);
                        c.esi=r!(c.ecx,0);put!(c.eax,8,c.esi);
                        c.esi=r!(c.ecx,4);
                    } else {
                        put!(c.eax,0,c.esi);c.esi=r!(c.ecx,4);put!(c.eax,4,c.esi);
                        c.esi=r!(c.ecx,8);put!(c.eax,8,c.esi);c.esi=r!(c.ecx,12);
                    }
                    put!(c.eax,12,c.esi);c.eax=c.eax.wrapping_add(16);
                }
                c.edx=c.edx.wrapping_add(2);c.ecx=c.ecx.wrapping_add(16);
                if (c.edx as i32)>=(c.edi as i32){break;}
            }
        }
        c.ecx=4;c.eax=1;w!(0x14,c.ecx);w!(0x34,c.eax);
        if (c.ebp as i32)>=(c.ecx as i32) {
            loop {
                if c.eax!=0 {c.esi=s!(0x44);w!(0x10,c.ebx);}
                else {c.eax=s!(0x44);c.esi=c.ebx;w!(0x10,c.eax);c.ebx=c.eax;}
                w!(0x18,0);
                if (c.edi as i32)>0 {
                    if (c.ecx as i32)<32 {
                        // Merge short runs; the two exhausted tails use ordered word loops.
                        c.eax=c.ecx;c.edx=((c.eax as i32)>>31)as u32;
                        c.eax=((c.eax.wrapping_sub(c.edx)as i32)>>1)as u32;
                        c.edx=c.eax;w!(0x2c,c.eax);c.eax=c.ecx.wrapping_mul(8);
                        w!(0x30,c.edx);w!(0x3c,c.eax);w!(0x28,c.ebx);
                        loop {
                            c.ebp=s!(0x18);w!(0x20,c.ebp);w!(0x24,c.edx);
                            c.eax=c.esp.wrapping_add(if (c.edi as i32)<(c.edx as i32){0x48}else{0x24});
                            c.edi=r!(c.eax,0);c.eax=s!(0x2c);c.eax=c.eax.wrapping_add(c.edi);
                            let clipped=(s!(0x48)as i32)<(c.eax as i32);
                            w!(0x24,c.eax);c.ebx=c.edi;
                            c.eax=c.esp.wrapping_add(if clipped{0x48}else{0x24});c.eax=r!(c.eax,0);
                            c.edx=c.eax.wrapping_sub(s!(0x18));w!(0x24,0);w!(0x38,c.edx);
                            if (c.edx as i32)>0 {
                                c.ecx=s!(0x10);c.edx=s!(0x28);c.ecx=c.ecx.wrapping_add(c.edi.wrapping_mul(8));
                                loop {
                                    if (c.ebp as i32)>=(c.edi as i32) {
                                        if (c.ebx as i32)<(c.eax as i32) {
                                            c.edx=s!(0x10);c.eax=c.eax.wrapping_sub(c.ebx);
                                            c.ecx=c.edx.wrapping_add(c.ebx.wrapping_mul(8));c.edx=c.eax;
                                            loop {
                                                c.eax=r!(c.ecx,0);put!(c.esi,0,c.eax);
                                                c.eax=r!(c.ecx,4);put!(c.esi,4,c.eax);
                                                c.esi=c.esi.wrapping_add(8);c.ecx=c.ecx.wrapping_add(8);c.edx=c.edx.wrapping_sub(1);
                                                if c.edx==0{break;}
                                            }
                                        }
                                        break;
                                    }
                                    if (c.ebx as i32)>=(c.eax as i32) {
                                        c.ecx=s!(0x10);c.ecx=c.ecx.wrapping_add(c.ebp.wrapping_mul(8));c.edi=c.edi.wrapping_sub(c.ebp);
                                        loop {
                                            c.edx=r!(c.ecx,0);put!(c.esi,0,c.edx);
                                            c.eax=r!(c.ecx,4);put!(c.esi,4,c.eax);
                                            c.esi=c.esi.wrapping_add(8);c.ecx=c.ecx.wrapping_add(8);c.edi=c.edi.wrapping_sub(1);
                                            if c.edi==0{break;}
                                        }
                                        break;
                                    }
                                    c.ebp=r!(c.edx,0);
                                    if c.ebp>r!(c.ecx,0) {
                                        c.ebp=r!(c.ecx,0);put!(c.esi,0,c.ebp);c.ebp=r!(c.ecx,4);put!(c.esi,4,c.ebp);
                                        c.esi=c.esi.wrapping_add(8);c.ebx=c.ebx.wrapping_add(1);c.ecx=c.ecx.wrapping_add(8);
                                    } else {
                                        w!(0x20,s!(0x20).wrapping_add(1));put!(c.esi,0,c.ebp);
                                        c.ebp=r!(c.edx,4);put!(c.esi,4,c.ebp);c.esi=c.esi.wrapping_add(8);c.edx=c.edx.wrapping_add(8);
                                    }
                                    c.ebp=s!(0x38);w!(0x24,s!(0x24).wrapping_add(1));
                                    if (s!(0x24)as i32)>=(c.ebp as i32){break;}
                                    c.ebp=s!(0x20);
                                }
                                c.ecx=s!(0x14);
                            }
                            c.edi=s!(0x3c);c.eax=s!(0x18);c.edx=s!(0x30);
                            w!(0x28,s!(0x28).wrapping_add(c.edi));c.edi=s!(0x48);
                            c.eax=c.eax.wrapping_add(c.ecx);c.edx=c.edx.wrapping_add(c.ecx);
                            w!(0x18,c.eax);w!(0x30,c.edx);
                            if (c.eax as i32)>=(c.edi as i32){break;}
                        }
                    } else {
                        // Large runs keep the original memcpy boundary and CPU state.
                        c.eax=c.ecx;c.edx=((c.eax as i32)>>31)as u32;
                        c.eax=((c.eax.wrapping_sub(c.edx)as i32)>>1)as u32;c.edx=c.eax;
                        w!(0x2c,c.eax);w!(0x20,c.edx);w!(0x30,c.ebx);
                        loop {
                            c.ebp=s!(0x18);w!(0x3c,c.edx);
                            let clipped=(c.edi as i32)<(c.edx as i32);w!(0x28,c.ebp);
                            c.edx=c.esp.wrapping_add(if clipped{0x48}else{0x3c});c.edx=r!(c.edx,0);
                            c.eax=c.eax.wrapping_add(c.edx);w!(0x3c,c.eax);
                            let clipped=(c.edi as i32)<(c.eax as i32);c.ebx=c.edx;
                            c.eax=c.esp.wrapping_add(if clipped{0x48}else{0x3c});c.edi=r!(c.eax,0);
                            c.eax=c.edi.wrapping_sub(s!(0x18));w!(0x24,0);w!(0x3c,c.eax);
                            if (c.eax as i32)>0 {
                                c.ecx=s!(0x10);c.eax=c.ecx.wrapping_add(c.edx.wrapping_mul(8));c.ecx=s!(0x30);
                                loop {
                                    if (c.ebp as i32)>=(c.edx as i32) {
                                        c.edx=s!(0x10);c.edi=c.edi.wrapping_sub(c.ebx);
                                        c.edi=c.edi.wrapping_add(c.edi);c.edi=c.edi.wrapping_add(c.edi);
                                        c.fl.alu_add(c.edi,c.edi,32);c.edi=c.edi.wrapping_add(c.edi);
                                        push(m,c,p,c.edi);c.eax=c.edx.wrapping_add(c.ebx.wrapping_mul(8));push(m,c,p,c.eax);
                                    } else if (c.ebx as i32)>=(c.edi as i32) {
                                        c.ecx=s!(0x10);c.fl.alu_sub(c.edx,c.ebp,32);c.edx=c.edx.wrapping_sub(c.ebp);
                                        c.edi=c.edx.wrapping_mul(8);push(m,c,p,c.edi);
                                        c.edx=c.ecx.wrapping_add(c.ebp.wrapping_mul(8));push(m,c,p,c.edx);
                                    } else {
                                        c.ebp=r!(c.ecx,0);
                                        if c.ebp>r!(c.eax,0) {
                                            c.ebp=r!(c.eax,0);put!(c.esi,0,c.ebp);c.ebp=r!(c.eax,4);put!(c.esi,4,c.ebp);
                                            c.esi=c.esi.wrapping_add(8);c.ebx=c.ebx.wrapping_add(1);c.eax=c.eax.wrapping_add(8);
                                        } else {
                                            w!(0x28,s!(0x28).wrapping_add(1));put!(c.esi,0,c.ebp);
                                            c.ebp=r!(c.ecx,4);put!(c.esi,4,c.ebp);c.esi=c.esi.wrapping_add(8);c.ecx=c.ecx.wrapping_add(8);
                                        }
                                        c.ebp=s!(0x3c);w!(0x24,s!(0x24).wrapping_add(1));
                                        if (s!(0x24)as i32)>=(c.ebp as i32){break;}
                                        c.ebp=s!(0x28);continue;
                                    }
                                    push(m,c,p,c.esi);push(m,c,p,0x00fee082);x86::call_ext(m,c,0x012efea0);
                                    c.esp=c.esp.wrapping_add(12);c.esi=c.esi.wrapping_add(c.edi);break;
                                }
                                c.ecx=s!(0x14);
                            }
                            c.edx=s!(0x18);c.edi=s!(0x48);w!(0x20,s!(0x20).wrapping_add(c.ecx));
                            c.edx=c.edx.wrapping_add(c.ecx);c.eax=c.ecx.wrapping_mul(8);
                            w!(0x30,s!(0x30).wrapping_add(c.eax));w!(0x18,c.edx);
                            if (c.edx as i32)>=(c.edi as i32){break;}
                            c.eax=s!(0x2c);c.edx=s!(0x20);
                        }
                    }
                    c.ebp=s!(0x1c);
                }
                c.eax=s!(0x34);c.ecx=c.ecx.wrapping_add(c.ecx);c.eax^=1;
                w!(0x14,c.ecx);w!(0x34,c.eax);
                if (c.ecx as i32)>(c.ebp as i32){break;}
                c.ebx=s!(0x4c);
            }
            use_input=c.eax==0;
            if !use_input{c.ebx=s!(0x4c);}
        }
    }
    if use_input{c.eax=s!(0x44);}
    c.edi=pop(m,c,p);c.esi=pop(m,c,p);c.ebp=pop(m,c,p);
    if !use_input{c.eax=c.ebx;}
    c.ebx=pop(m,c,p);
    c.fl.alu_add(c.esp,0x30,32);c.esp=c.esp.wrapping_add(0x40);
}
