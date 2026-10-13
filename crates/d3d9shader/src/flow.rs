//! Where a pixel shader's interpolated inputs flow (L113).
//!
//! [`interpolants_only_address`] tells whether the interpolated inputs
//! (`v#`, and `vPos`/`vFace`) reach nothing but the coordinates of texture
//! fetches: no output, no branch, no texkill, no fetch gradient or bias, no
//! relative addressing. For such a shader, every result bit depends on the
//! interpolants only through which texels the fetches read. With point
//! sampling and no mip selection, any interpolated value inside a pixel's
//! texel reads the same texel, so the shader can run where the
//! interpolation is computed differently (a compute dispatch over the
//! target) with bit-identical results. The caller checks the samplers.
//!
//! The analysis taints register components: an instruction's destination
//! is tainted by any tainted source component its swizzles select. The two
//! sides of an `if` are analysed separately and joined; a loop's body is
//! analysed until the taint at its head stops growing.
use crate::{op, reg, Instruction, Shader, Stage};
use std::collections::HashMap;

/// What the interpolants address, when they reach nothing else.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Addressing {
    /// Samplers whose fetch coordinates depend on the interpolants (bit n:
    /// sampler n). These must sample a single texel per coordinate (point
    /// filtering, no mip selection) for the shader to be independent of
    /// how the interpolants are computed.
    pub samplers: u32,
    /// Samplers fetched with an implicit level (texld, texldb): their level
    /// would come from derivatives; with a single level or no mip filter it
    /// does not matter.
    pub implicit_level: u32,
}

fn tainted(t: &HashMap<(u8, u32), [bool; 4]>, p: &crate::Param, inputs_tainted: bool) -> bool {
    match p.kind {
        reg::INPUT | reg::MISCTYPE => inputs_tainted,
        reg::TEMP | reg::PREDICATE | reg::ADDR | reg::LOOP => {
            let Some(c) = t.get(&(p.kind, p.index)) else { return false };
            // the components the swizzle selects
            (0..4).any(|k| c[((p.mask_or_swizzle >> (2 * k)) & 3) as usize])
        }
        _ => false,
    }
}

type Taint = HashMap<(u8, u32), [bool; 4]>;

/// See the module documentation: Ok with the addressed samplers, or the
/// first reason the interpolants reach something else.
pub fn interpolants_only_address(s: &Shader) -> Result<Addressing, String> {
    if s.stage != Stage::Pixel {
        return Err("not a pixel shader".into());
    }
    let mut a = Analysis { ins: &s.instructions, out: Addressing::default(), breaks: Vec::new() };
    a.run(0, s.instructions.len(), Taint::new())?;
    Ok(a.out)
}

struct Analysis<'a> {
    ins: &'a [Instruction],
    out: Addressing,
    /// the states at the breaks of each loop being analysed (innermost last)
    breaks: Vec<Vec<Taint>>,
}

fn join(a: &Taint, b: &Taint) -> Taint {
    let mut j = a.clone();
    for (k, c) in b {
        let e = j.entry(*k).or_insert([false; 4]);
        for i in 0..4 {
            e[i] |= c[i];
        }
    }
    j
}

impl Analysis<'_> {
    /// The index of the instruction that closes the block opened at `at`
    /// (else or endif for an if, endloop or endrep for a loop), and the else.
    fn block_end(&self, at: usize) -> Result<(Option<usize>, usize), String> {
        let mut depth = 0;
        let mut else_at = None;
        for i in at + 1..self.ins.len() {
            match self.ins[i].opcode {
                op::IF | op::IFC | op::LOOP | op::REP => depth += 1,
                op::ELSE if depth == 0 => else_at = Some(i),
                op::ENDIF | op::ENDLOOP | op::ENDREP if depth == 0 => return Ok((else_at, i)),
                op::ENDIF | op::ENDLOOP | op::ENDREP => depth -= 1,
                _ => {}
            }
        }
        Err(format!("instruction {at}: unclosed block"))
    }

    /// The taint after instructions `from..to` from `state`.
    fn run(&mut self, from: usize, to: usize, mut state: Taint) -> Result<Taint, String> {
        let mut at = from;
        while at < to {
            let ins = &self.ins[at];
            match ins.opcode {
                op::IF | op::IFC => {
                    let cond = ins.src.iter().any(|p| tainted(&state, p, true))
                        || ins.predicate.as_ref().is_some_and(|p| tainted(&state, p, true));
                    if cond {
                        return Err(format!("instruction {at} (opcode {}): a branch on an interpolant", ins.opcode));
                    }
                    let (else_at, end) = self.block_end(at)?;
                    let then = self.run(at + 1, else_at.unwrap_or(end), state.clone())?;
                    let other = match else_at {
                        Some(e) => self.run(e + 1, end, state)?,
                        None => state,
                    };
                    state = join(&then, &other);
                    at = end + 1;
                }
                op::LOOP | op::REP => {
                    if ins.src.iter().any(|p| tainted(&state, p, true)) {
                        return Err(format!("instruction {at} (opcode {}): a loop on an interpolant", ins.opcode));
                    }
                    let (_, end) = self.block_end(at)?;
                    // the loop head: the state before it joined with every
                    // iteration's end, until it stops growing
                    let mut head = state;
                    self.breaks.push(Vec::new());
                    loop {
                        let body = self.run(at + 1, end, head.clone())?;
                        let next = join(&head, &body);
                        if next == head {
                            break;
                        }
                        head = next;
                    }
                    let breaks = self.breaks.pop().unwrap_or_default();
                    state = breaks.iter().fold(head, |s, b| join(&s, b));
                    at = end + 1;
                }
                op::BREAK | op::BREAKC | op::BREAKP => {
                    step(ins, at, &mut state, &mut self.out)?;
                    if let Some(b) = self.breaks.last_mut() {
                        b.push(state.clone());
                    }
                    at += 1;
                }
                _ => {
                    step(ins, at, &mut state, &mut self.out)?;
                    at += 1;
                }
            }
        }
        Ok(state)
    }
}

fn step(ins: &Instruction, at: usize, taint: &mut Taint, out: &mut Addressing) -> Result<(), String> {
    let src_taint = |i: usize| ins.src.get(i).is_some_and(|p| tainted(taint, p, true));
    let any_src = (0..ins.src.len()).any(src_taint);
    let fail = |what: &str| Err(format!("instruction {at} (opcode {}): {what}", ins.opcode));
    // relative addressing by a tainted register
    for p in ins.src.iter().chain(ins.dst.iter()) {
        if let Some(r) = &p.relative {
            if tainted(taint, r, true) {
                return fail("relative addressing by an interpolant");
            }
        }
    }
    let predicate = ins.predicate.as_ref().is_some_and(|p| tainted(taint, p, true));
    let dest_taint = match ins.opcode {
        op::COMMENT | op::NOP | op::DCL | op::DEF | op::DEFI | op::DEFB | op::PHASE | op::END | op::LABEL | op::RET => return Ok(()),
        // (if, loop and rep are run() blocks)
        op::ELSE | op::ENDIF | op::ENDLOOP | op::ENDREP | op::BREAK => return Ok(()),
        op::BREAKC | op::BREAKP | op::CALLNZ => {
            if any_src || predicate {
                return fail("a break or call on an interpolant");
            }
            return Ok(());
        }
        op::CALL => return fail("subroutines are not analysed"),
        op::TEXKILL => {
            if any_src || predicate {
                return fail("texkill on an interpolant");
            }
            return Ok(());
        }
        op::DSX | op::DSY => return fail("derivatives"),
        op::TEX | op::TEXLDL | op::TEXLDD => {
            // src0 the coordinates, src1 the sampler; texldd's gradients
            let sampler = ins.src.get(1).map_or(0, |p| p.index);
            if src_taint(0) {
                out.samplers |= 1 << sampler;
            }
            if ins.opcode == op::TEX {
                out.implicit_level |= 1 << sampler;
            }
            if ins.opcode == op::TEXLDD && (src_taint(2) || src_taint(3)) {
                return fail("fetch gradients from an interpolant");
            }
            // texldl's level and texldb's bias are coordinate w: with a
            // single level they select nothing (the sampler check)
            // the fetched value does not depend on the interpolants beyond
            // the texel choice; a tainted predicate still selects whether
            // the destination is written
            predicate
        }
        _ => any_src || predicate,
    };
    let Some(d) = &ins.dst else { return Ok(()) };
    match d.kind {
        reg::COLOROUT | reg::DEPTHOUT => {
            if dest_taint {
                return fail("an output from an interpolant");
            }
        }
        reg::TEMP | reg::PREDICATE | reg::ADDR | reg::LOOP => {
            let mask = d.mask_or_swizzle & 0xF;
            let e = taint.entry((d.kind, d.index)).or_insert([false; 4]);
            for (k, c) in e.iter_mut().enumerate() {
                if mask & (1 << k) != 0 {
                    *c = dest_taint;
                }
            }
        }
        _ => {}
    }
    Ok(())
}
