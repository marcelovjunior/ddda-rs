# L37: D3DX shader reflection (constant tables and output semantics)

- **Owner:** claude-5
- **Branch:** `task/L37-d3dx` (worktree `E:\ddda-rs-wt\L37`)
- **Rules:** [AGENT-RULES.md](AGENT-RULES.md)

## Why

`docs/TITLE_PATH.md` (L35) found that the first draw of the title screen
links a shader pair (`sShader2` → 0x00E12850; the linked-program code
0x00E12A20 → 0x00E10690). That calls two D3DX9_43 imports:
- `D3DXGetShaderConstantTableEx` (flags 0x20000), which returns an
  `ID3DXConstantTable` COM object;
- `D3DXGetShaderOutputSemantics`.

Both read only the shader bytecode: the CTAB comment block, and the
vertex shader's output declarations. They are host-independent, and
`d3dx9_43.dll` is on this machine as an oracle that needs no D3D9 device.

## Task

1. **Census:** every D3DX import the game calls (address, flags) and
   every `ID3DXConstantTable` method it uses (vtable slots, call sites).
2. **Implement** in `crates/ddruntime`:
   - the two functions;
   - `ID3DXConstantTable`, with exactly the methods the game uses. Build
     them on `crates/d3d9shader`'s decoder (CTAB parsing).
   - Return layouts must be byte-exact: the constant descriptions,
     handles, and the buffer behind GetBufferPointer if used.
3. **Check against the real d3dx9_43.dll** on every one of the game's
   29,198 shaders (no device needed: call the DLL directly from a test
   harness, the way `d3d9ref` calls d3d9.dll). Compare every field the
   game reads. Record any shader whose answer differs and why.
4. Results and the RUNTIME.md row. Merge-train rule.

## Results

### 1. Census (Ghidra)

The exe imports three D3DX9_43 functions (thunks 0x012E1B66..0x012E1B72),
each called from one place:

| Import | Call site | Use |
|---|---|---|
| D3DXGetShaderConstantTableEx | 0x00E106BF in `FUN_00E10690`, flags 0x20000 (D3DXCONSTTABLE_LARGEADDRESSAWARE) | the game's VS/PS pair linker (sShader2::vf06 → 0x00E12A20 → 0x00E12850 → 0x00E10690, per L35's TITLE_PATH.md) |
| D3DXGetShaderOutputSemantics | 0x00E10F6A in `FUN_00E10690`, vertex shaders only | same |
| D3DXSaveTextureToFileInMemory | 0x00DA95C9 in `FUN_00DA9580` | only on a screenshot/capture request (sRender+0x440E10/+0x440E14); not implemented here |

`ID3DXConstantTable` methods the game uses (vtable per d3dx9shader.h):
- GetDesc (slot 5, +0x14) for the constant count;
- GetConstant(NULL, i) (slot 8, +0x20) for every top-level constant;
- GetConstantDesc(handle, &desc, &count) (slot 6, +0x18), called once
  with an unset count and once with count 0;
- Release (slot 2).

The game reads each description's Name (skipping names starting with
"__"), RegisterSet, RegisterIndex, RegisterCount and Type, and handles
samplers (register set 3) separately.

### 2. Implementation

- **`crates/d3d9shader/src/ctab.rs`:** the CTAB comment block
  (D3DXSHADER_CONSTANTTABLE, CONSTANTINFO, TYPEINFO) decoded into what
  D3DX reports:
  - the creator, version and constants;
  - per constant, every D3DXCONSTANT_DESC field. `Bytes` is computed from
    the type information: 4 bytes per component times the elements,
    structures summed, samplers 4.
  - default-value offsets;
  - `output_semantics`: the vertex shader's dcl_ outputs (usage, index)
    in declaration order.
- **`crates/ddruntime/src/d3dx.rs`:** the two imports and
  `ID3DXConstantTable` (an object kind in the D3D9 object registry, with
  its own vtable).
  - The table keeps a guest copy of the CTAB block, so Name, Creator and
    DefaultValue point into guest memory, as the DLL's point into its copy.
  - A handle is the guest address of the constant's CONSTANTINFO record.
  - Methods the game does not use stop with their name, as do flags other
    than 0x20000 and GetConstant with a parent handle.
  - Registrations: `pub mod d3dx;` in lib.rs, the import table in
    services.rs, and the vtable in d3d9dev's install.
- **Measured DLL behaviours, reproduced:**
  - GetConstantDesc with `*pCount = 0` still writes the description and a
    count of 1 (all 818,684 constants);
  - GetConstant past the last constant gives NULL;
  - a shader without a constant table gives D3DXERR_INVALIDDATA
    (0x88760B59).

### 3. Against d3dx9_43.dll

- `crates/d3dxref` calls the host's 64-bit d3dx9_43.dll directly; no D3D9
  device is involved.
- `crates/d3dxref/tests/game_shaders.rs` compares **all 29,198 distinct
  shaders of every package**. All have constant tables, and **0 differ**
  in the table description, every field of every constant's description
  including default values, or the vertex output semantics.
- `crates/ddruntime/tests/d3dx.rs` calls the guest imports as the game
  does, on the Title package's shaders, and compares every answer with
  the DLL. It is not built on this branch under the batched-verification
  rule (`cargo check` only); the integration build runs it.

### 4. Verification status

- **Verified:** `cargo test -p d3dxref` (all game shaders against the DLL,
  edge cases).
- **For the integration build:** the ddruntime side (`d3dx.rs`, the
  registrations, `tests/d3dx.rs`).
