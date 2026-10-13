Generator of crates/mtcore/src/semantic_s5/kernels.rs (L184 s5).
Needs the Ghidra server (127.0.0.1:8089, DDDA loaded) and the unpacked exe for constants.
Run from this directory with E:/ddda-re/tools/pyenv/Scripts/python.exe:
  python gen_864660.py; python gen_865990.py; python assemble_kernels.py
sym.py   symbolic executor: follows the data flow of the scalar/packed SSE float code, both arms of every float
         compare are executed and merged with 'sel' nodes; the result per region is a pure function.
emit.py  turns the node DAG into Rust (shared nodes become lets, loaders list the guest loads).
Each kernel is validated against the original's code by the Unicorn vectors (research/native/record_cns_*.py).
