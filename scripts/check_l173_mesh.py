"""Head-owned draw_mesh versus EA5320 vectors; compile no game or shards.

Reads the head branch's source into private scratch, never edits that branch.
Runs its ABI harness with the mesh call arities and the known heap-comparison
fix, then the L173 harness (also checks heap at every call and sphere data).
"""
import os
from pathlib import Path
import subprocess

def head_file(revision,path):
    return subprocess.check_output(['git','show',revision+':'+path],text=True)

def main():
    repo=Path(__file__).resolve().parents[1]
    out=Path('E:/ddda-re/notes/native-L173/mesh-isolated');out.mkdir(parents=True,exist_ok=True)
    revision=subprocess.check_output(['git','rev-parse','task/L171-semantic'],text=True).strip()
    semantic=head_file(revision,'crates/mtcore/src/semantic.rs')
    start=semantic.index('pub(crate) fn call_from(');end=semantic.index('\n}\n',start)+3
    (out/'semantic.rs').write_text('use ddnative_base::{mem::{Mem,Pages},x86::Cpu};\n'+semantic[start:end])
    (out/'semantic_draw.rs').write_text(head_file(revision,'crates/mtcore/src/semantic_draw.rs'))
    test=head_file(revision,'crates/mtcore/tests/semantic.rs').replace('//!','//')
    while '#[test]' in test:
        start=test.index('#[test]');end=test.index('\n}\n',start)+3
        test=test[:start]+test[end:]
    test=test.replace('if a >= esp {','if !(0x0f000000..esp).contains(&a) {')
    test=test.replace('match target {','''match target {
        0xfed940|0xff14d0|0xfeef20|0xff1920=>0,
        0xea42e0=>3,0xfed190|0xea23f0=>2,0xfed890=>1,0x1200000=>4,''')
    test+='''
mod semantic;
mod semantic_draw;
#[test] fn head_mesh_abi_cases_with_calls(){ddnative_host::install();let count=abi_cases_with_calls_ret("E:/ddda-re/notes/native-L171/draw_mesh.json",semantic_draw::draw_mesh,false);eprintln!("mesh {count} head ABI cases");}
'''
    strict=(repo/'crates/mtcore/tests/semantic_draw_a.rs').read_text().split('// verifies')[0].replace('//!','//')
    strict=strict.replace('format!("E:/ddda-re/notes/native-L173/{entry:08x}.json")','"E:/ddda-re/notes/native-L171/draw_mesh.json".to_string()')
    strict=strict.replace('cases.len() >= 160, "complete L173 leaf corpus required"','cases.len() >= 302, "complete EA5320 corpus required"')
    # This function is void; unlike the leaf suite, do not require incidental
    # EAX. The head harness explicitly uses returns=false for this contract.
    strict=strict.replace('[c.eax, c.ebx,','[0, c.ebx,').replace('[after[0], after[3],','[0, after[3],')
    strict=strict.replace('let recorded = pairs(&call["memory"]);','''let recorded = pairs(&call["memory"]);
        let sphere=if target==0x1200000 {n(&call["args"][3])}else{0};''')
    strict=strict.replace('if !(0x0f000000..0x0f010000).contains(&a) ||','if (sphere!=0 && (sphere..sphere+16).contains(&a)) || !(0x0f000000..0x0f010000).contains(&a) ||')
    strict+='''
#[test] fn strict_mesh(){verify(0xea5320,super::semantic_draw::draw_mesh);}
#[test] fn strict_lifted_mesh(){x86::set_lifted_lookup(|a|match a {0xfed940=>Some(|m,c|x86::call_ext(m,c,0xfed940)),_=>None});verify(0xea5320,super::lifted::f_00ea5320);}
'''
    test+='\nmod strict {\n'+strict+'}\n'
    generated=(repo/'crates/ddlift/lift_29/src/generated/lifted_010.rs').read_text()
    start=generated.index('pub fn f_00ea5320(');end=generated.find('\n// Translated from ',start)
    body=generated[start:end if end>=0 else len(generated)].replace('crate::mem::','ddnative_base::mem::')
    test+='\nmod lifted {\n#![allow(warnings)]\nuse ddnative_base::{mem::Mem,emit,x86::{self,Cpu,Flags}};\n'+body+'\n}\n'
    (out/'isolated.rs').write_text(test)
    (out/'Cargo.toml').write_text(f'''[package]
name="l173-mesh-isolated"
version="0.1.0"
edition="2021"
[workspace]
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
ddnative-host={{path="{repo.as_posix()}/crates/ddnative-host"}}
serde_json="1"
[[test]]
name="isolated"
path="isolated.rs"
''')
    print(f'head source revision: {revision}',flush=True)
    env=dict(os.environ,CARGO_INCREMENTAL='0',CARGO_TARGET_DIR='C:/ddda-targets/L173',CARGO_BUILD_JOBS='4',DDNATIVE_REQUIRE_VECTORS='1')
    command=['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--release']
    command+=['--no-run'] if os.getenv('L173_MESH_COMPILE_ONLY') else ['--','--nocapture','--test-threads=1']
    subprocess.run(command,env=env,check=True)

if __name__=='__main__':main()
