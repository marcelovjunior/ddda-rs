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

(open)
