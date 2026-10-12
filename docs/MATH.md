# The MT Framework math library (readable ports)

`crates/mtmath` holds readable Rust for the out-of-line math helpers of
DDDA.exe (MT Framework's MtMatrix and MtRandom, and the game's own angle
and vector helpers): 29 functions with 1,497 direct call sites in 786
calling functions.
Each function is bit for bit the original's, verified against it, and the
lifted program calls it instead of the lifted body. This page is for
modders and porters: types, layouts, conventions, how the wiring works,
how it is verified, and what is still lifted. Brief and results:
[tasks/L24-readable-math.md](tasks/L24-readable-math.md).

## What the exe contains

Most of MT Framework's vector and quaternion code is inline: MtVector3
arithmetic, dot and cross products and quaternion operations are
expanded into their callers by the compiler, so they have no address of
their own. What remains out of line was found by shape
(`research/native/math_census.py`: small functions that are mostly SSE or
x87 instructions, ranked by direct callers), then read in Ghidra.

| address | callers | what | state |
|---|---:|---|---|
| 0x00D00690 | 386 | MtRandom::next (xorshift128 step) | ported, wired |
| 0x00D006C0 | 4 | MtRandom::seed | ported, wired |
| 0x007AEE90 | 12 | random float in [a, b) from the global MtRandom (x87) | lifted |
| 0x00CFC1D0 | 14 | MtMatrix from Euler angles + translation (CRT sinf/cosf) | ported, wired |
| 0x00CFC350 | 12 | perspective from field of view, RH (CRT tanf) | ported, wired |
| 0x00CFC4B0 | 3 | perspective from field of view, x/y scaled (CRT tanf) | ported, wired |
| 0x00CFC620 | 2 | perspective from field of view, LH (CRT tanf) | ported, wired |
| 0x00CFC770 | 1 | perspective from size, RH | ported, wired |
| 0x00CFC810 | 1 | perspective from size, LH | ported, wired |
| 0x00CFC8B0 | 12 | orthographic, RH | ported, wired |
| 0x00CFC940 | 3 | orthographic, LH | ported, wired |
| 0x00CFC9E0, 0x00CFCB00, 0x00CFCC20, 0x00CFCD40, 0x00CFCE60, 0x00CFCF80 | 12, 5, 5, 5, 6, 45 | MtMatrix to Euler angles, one per rotation order (CRT atan2, asinf) | ported, wired |
| 0x00CFD0A0 | 2 | multiply by an Euler rotation (calls 0x00CFC1D0) | lifted |
| 0x00CFD5E0 | 35 | look-at, RH | ported, wired |
| 0x00CFD890 | 5 | look-at, LH | ported, wired |
| 0x00CFDB10 | 2 | reflection in a plane | ported, wired |
| 0x00CFDC80 | 10 | matrix blend (normalized rows lerped, scales lerped) | lifted (see below) |
| 0x007AD870 | 46 | difference of two angles, short way round | ported, wired |
| 0x007AD9D0 | 82 | yaw from one point to another: atan2(dx, dz) wrapped to [-pi, pi) | ported, wired |
| 0x007AD7A0 | 65 | the same over the normalized 3D direction | ported, wired |
| 0x007AD8E0 | 66 | the point (G.x, G.y, d) rotated about y; G is the global vector at 0x018CFE80 (CRT sinf/cosf) | ported, wired |
| 0x007AEED0 | 50 | unit normal of b x a, falling back to a second pair | ported, wired |
| 0x007AF5B0 | 31 | orthonormal frame from a forward vector (calls 0x007AEED0) | ported, wired |
| 0x007AF450 | 13 | orthonormal frame from an up vector (calls 0x007AEED0) | ported, wired |
| 0x007ADCA0 | 17 | target inside a horizontal view cone (CRT sinf/cosf) | ported, wired |
| 0x007ADC00 | 5 | angle between two vectors (CRT acosf) | ported, wired |
| 0x007AE860 | 15 | quaternion turned toward a target by at most an angle (CRT acosf/cosf/sinf) | ported, wired |
| 0x00D4EBF0, 0x00D4EC50 | 25, 49 | cubic curve evaluation (x87) | lifted |

The MtMatrix translation unit is 0x00CFC1D0..0x00CFDC80 (it sits between
MtPropertyList and MtThread); the identity matrix it copies is at
0x016295C0. MtRandom is at 0x00D00690..0x00D00839; the game's global
generator object is at 0x018D017C.

The CRT's SSE2 float routines these call are platform code (lifted, not
ported): 0x012F0906 sinf, 0x012F0A55 cosf, 0x012F0BAC tanf, 0x012F0D4C
asinf, 0x012F161F acosf (float in and out in xmm0), and 0x012F1109 atan2,
0x012F1E8A sin, 0x012F2042 cos (doubles in xmm0/xmm1). Identified by
running them in Unicorn on known inputs.

## Types and layouts

All `#[repr(C)]`, at the original's offsets.

| Rust | original | size | layout |
|---|---|---:|---|
| `F32` | `float` | 4 | one IEEE single (see "Exact floats") |
| `Vec3` | MtVector3 | 16 | x +0, y +4, z +8, padding +0xC |
| `Plane` | MtPlane | 16 | a, b, c, d: `a x + b y + c z + d = 0` |
| `Mat44` | MtMatrix | 64, align 16 | row-major, `m[r][c]` at `0x10 r + 4 c` |
| `quat::Quat` | MtQuaternion | 16 | x, y, z (vector part), w |
| `random::Random` | MtRandom | 16 | words x, y, z, w |

Conventions follow Direct3D, as the original does:

- row vectors: a point `p` maps to `p * M`; rows 0..2 of a matrix are the
  transformed x, y, z axes, row 3 the translation;
- RH/LH builders match the D3DX functions of the same name (perspective,
  ortho, look-at, reflect);
- angles in radians; 2pi is 0x40C90FDB, pi 0x40490FDB;
- a vector is normalized only if its length is at least FLT_EPSILON
  (0x34000000); shorter vectors are used as they are;
- MtVector3's length is `sqrt((y*y + x*x) + z*z)`, in that order (a
  few helpers sum x first; each port keeps its own order);
- quaternion products are Hamilton products, `q * s` applies `s` first.

## Exact floats

The ports compute in single precision, in the original's operation order,
with no fused multiply-add. Plain Rust `f32` would almost do; the one gap
is that LLVM may swap the operands of `+` and `*`, and when both operands
are NaN, SSE returns the first one (quieted), so a swap can change a NaN's
payload. `F32` pins each `+ - * /` to one SSE instruction with the written
operand order (inline assembly, `options(pure, nomem, nostack)`, so the
compiler still schedules and eliminates it). `scalar::tests` checks the
rule on the host CPU. Comparisons translate the original's `comiss` +
branch exactly: `jbe` after `comiss a, b` is `!(a > b)`, so NaN takes the
same branch as in the original.

When reading or changing a port, keep the operand order: `inv * d.x` and
`d.x * inv` are different functions when two different NaNs meet.

## Wiring: how the lifted program calls the ports

Every port has an adapter in `crates/mtmath/src/abi.rs` with a lifted
function's signature (`fn(&mut dyn Mem, &mut Cpu)`, return address at
`[esp]`), listed in its `PORTS` table. `research/native/lift.py gen` reads
that table and, in the shard that holds the address, writes

```rust
pub use mtmath::abi::f_00d00690;          // shadows the lifted body's glob import
pub mod replaced { pub use super::lifted_000::f_00d00690; }   // the lifted body, kept
```

into `generated/mod.rs`. In Rust an explicit import shadows a glob
import, so the shard's own calls (`super::lifted::f_X`), its `FUNCTIONS`
table and `lookup()` all resolve to the port. Cross-shard calls go through
`x86::call_lifted`, which uses `lookup()`, and the runtime's dispatcher
(`ddruntime::dispatch::Registry::with_lifted_program`) binds
`lifted::all()`, which is built from `FUNCTIONS`. Nothing else changes:
lifted bodies, vectors and the recorder are untouched, and `replaced::f_X`
keeps each lifted body for comparison.

An adapter leaves the machine as the original's `ret` does, as far as the
lifted contract observes it: all eight general registers (including the
scratch values the original leaves in EAX/ECX/EDX), xmm0, the x87 state,
and every byte the original writes, including pushes and spills below
ESP (the look-at builders spill to an aligned frame; the seed counts its
loop in its argument slot). `PORTS` also records which other xmm registers
each adapter reproduces exactly: all of them for look-at, reflect and the
generator, xmm0..xmm4 for the angle difference.

The other xmm registers keep their entry values, which is only equivalent
if no caller reads a scratch value there. `research/native/math_liveness.py`
checks this for every direct call site: it walks each caller from the
return address, lane by lane (32-bit lanes; a scalar write leaves the other
lanes pending without reading them), into direct callees, until every
pending lane is written. It found one real case: the callers of 0x007AD870
go on to the CRT's `atan2`/`sin`/`cos`, whose packed operations carry the
upper lanes of xmm2 and xmm4 along. That adapter therefore rebuilds
xmm0..xmm4. With that, 0 problems over 1,497 call sites; Ghidra lists
only direct calls to the ported functions.

### Calls into code that is not ported

Sixteen of the ports call the CRT's float routines, which are not lifted
(the lifted bodies reach them through `x86::call_ext`, the runtime's
dispatcher). The readable function takes the routine as a closure or a
small trait (`angle::yaw_between(from, to, atan2)`,
`euler::EulerInput`), so it reads as math. The adapter passes a closure
that makes the same call the lifted body makes: it puts the machine into
the original's state at that call (registers, pushes, spills, the
return address), calls `x86::call_ext`, and continues from whatever the
callee left. Two ports call another port (0x007AF5B0 and 0x007AF450 call
0x007AEED0); their adapters call it the same way, on the machine.

Some originals read an input again after writing part of the output
(0x007AF5B0 re-reads its forward vector after rows 0 and 1). The readable
function is written for separate inputs and outputs; the adapter performs
the reads and writes in the original's order, and the edge vectors
include inputs that point into the output. At run time this is the dispatcher, exactly as for the
lifted body; in the tests it is a recorded replay (below).


## Verification

- `crates/mtmath/tests/differential.rs`, `ports_replay_the_lifted_vectors`:
  the lifted program's own vectors for each ported address (3 each, 87 in
  all, copied from `vectors/lift.json` into `vectors/math_lift.json`),
  under the lifted test's contract (registers, xmm0, exactly the written
  bytes, no read of a byte the original did not read; calls to unported
  code are replayed from the vector, with every register checked at the
  call).
- `ports_match_the_original_on_edge_cases`: 300 cases per function
  recorded from the original in Unicorn (`research/native/vectors_math.py
  edge`): zeros, signed zeros, denormals, huge values, infinities, quiet
  and signalling NaNs with several payloads, degenerate geometry (eye ==
  at, up along the view), random states, every ESP alignment. Objects and
  the stack window are filled with garbage and compared byte for byte; all
  general registers and the reproduced xmm registers, all lanes. Unicorn
  applies the x87 rule when an SSE op meets two NaNs; the recorder
  replaces those result lanes with the hardware's (first operand,
  quieted) and counts them (815 of 8,700 cases).
  Calls to the CRT (`CALLOUTS` in the recorder) run the original CRT in
  Unicorn; the case records the machine at the call and at the return
  (registers, all xmm lanes, the stack window). The test checks every
  register and xmm lane at the adapter's call and restores the recorded
  return state, so the readable code around the call is compared and the
  CRT is treated as the opaque callee it is (4,800 cases make such calls).
  Globals a function reads (the base matrix 0x016295C0, the vector
  0x018CFE80) get random values and are recorded as windows.
- The generated `crates/ddnative/tests/lifted.rs` replays all of
  `vectors/lift.json` through `lifted::all()`, which now holds the ports,
  so the full required-vector replay covers them too.
- Mutations are caught: pi off by one ulp (2 cases), one swapped operand
  in look-at LH (3 cases), one sign in the Euler table (52 cases).
- Branch coverage is counted per function where it matters (the Euler
  conversions' main and pole branches: 44 to 194 cases each; the unit
  normal's three paths: 184/41/75; rotate-toward's early exit, zero axis
  and rotation: 44/25/231).

To regenerate after a change: `PY research/native/vectors_math.py lift`,
`PY research/native/vectors_math.py edge`, `PY research/native/lift.py gen`,
then `scripts/native_gate.sh` (runs `cargo test -p mtmath` with required
vectors as well).

## Adding a port

1. Read the original (`harness.disasm`, Ghidra) and write the readable
   function in `crates/mtmath/src/<module>.rs` with its `// Translated
   from 0x... (DDDA.exe 2017-12-13 Steam), Name.` line, constants cited by
   address.
2. Write the adapter `f_xxxxxxxx` in `abi.rs`: arguments where the
   original reads them, registers and spills it leaves, `ret N`. Add it to
   `PORTS` with its exact-xmm mask.
3. Add the address to `PORTS` in `research/native/vectors_math.py` with
   an input kind (and any CRT routine it calls to `CALLOUTS`), record
   `lift` and `edge`, and add it to the `// verifies` lines of the tests.
4. Run `math_liveness.py`; if it reports a read, reproduce that register.
5. `lift.py gen`, then the gate.

## Changing behaviour (mods)

The lifted program calls the readable functions, so editing one changes
the game: for example `matrix::perspective_fov_rh` for a different
projection, or `random::Random::next` for another generator. Keep the
adapter's contract (which registers and bytes it leaves) unless you have
checked the callers with `math_liveness.py`. The differential tests will
then report every case that no longer matches the original: that is
their job, so a mod belongs behind its own switch, with the original
behaviour (and its passing tests) as the default.

## Not ported, and why

- 0x00CFD0A0 (multiply by an Euler rotation, 2 callers): calls
  0x00CFC1D0 and multiplies in a 0x90-byte aligned frame; same issue as
  the blend below, smaller value.
- 0x00CFDC80 (matrix blend): correct to port, but the original spills
  about 130 values to a 0x100-byte aligned frame, and an adapter has to
  replay that schedule. Ignoring dead stack below the caller's ESP is not
  an option: `research/native/stack_audit.py` finds 332 functions that
  read frame bytes they never wrote (e.g. 16-byte copies of partly written
  vectors), so stack a function leaves behind can reach later code.
- x87 functions (0x007AEE90, random float in a range; 0x00D4EBF0 and
  0x00D4EC50, cubic curves): they compute in x87 registers under the
  guest's precision control and return an extended value in st0. Written
  as Rust `f64` they would round differently under the 64-bit precision
  setting, and written with the x87 operations they would just be the
  lifted body again. Lifted.

## Performance

`cargo run --release -p mtmath-bench` times each wired function three
ways: the lifted body over guest memory, the adapter (what the lifted
program now runs) and the readable function on Rust values. See the
brief's Results for the measured numbers.
