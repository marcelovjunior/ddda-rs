"""Build isolated L130 lifted bodies and optional emulator benchmark, never shards."""
import argparse
import os
from pathlib import Path
import re
import subprocess


EMU_TEST = r'''
#[test]
#[ignore = "bounded runtime emulator versus native duration microbenchmark"]
fn durations_runtime_cost() {
    use ddruntime::{dispatch::Registry,memory::GuestMemory};
    ddnative_host::install();
    ddnative_base::mem::ATOMIC_ACCESSES.store(true,std::sync::atomic::Ordering::Relaxed);
    let data=duration_vectors().unwrap();
    let registry=Registry::new();
    let mut m=GuestMemory::new();
    for base in [0x112b000,0xe000000,0xf008000] {
        m.map(base,0x1000,ddnative_base::mem::READ|ddnative_base::mem::WRITE).unwrap();
    }
    for (address,bytes) in data["code"].as_object().unwrap() {
        let address:u32=address.parse().unwrap();
        for (i,b) in bytes.as_str().unwrap().as_bytes().chunks(2).enumerate() {
            m.set_u8(address+i as u32,u8::from_str_radix(std::str::from_utf8(b).unwrap(),16).unwrap());
        }
    }
    for case in data["cases"].as_array().unwrap().iter().filter(|c| n(&c["value"])==128 && n(&c["depth"])==3 && n(&c["rounding"])==0) {
        let entry=n(&case["entry"]);
        let initial=pairs(&case["initial"]);
        let initial_cpu=cpu(&case["before"]);
        for round in 0..5 {
            for offset in 0..2 {
                let emulated=(round+offset)%2==0;
                let mut elapsed=std::time::Duration::ZERO;
                for iteration in 0..1020 {
                    for (&a,&b) in &initial {m.set_u8(a,b);}
                    let mut c=initial_cpu.clone();
                    let start=std::time::Instant::now();
                    if emulated { ddruntime::emu::run(&registry,&mut m,&mut c,entry); }
                    else { duration_port(entry)(&mut m,&mut c); }
                    let time=start.elapsed();
                    if iteration>=20 {elapsed+=time;}
                    if iteration==0 {check(&m,&c,&case["after"],"runtime duration bench gate");}
                    release_x87(&mut c);
                }
                let name=if emulated {"runtime-emu"} else {"port"};
                eprintln!("runtime duration entry={entry:x} round={round} {name} ns/call={:.1}",elapsed.as_nanos() as f64/1000.0);
            }
        }
    }
}
'''


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--out',default='E:/ddda-re/notes/native-L130/check')
    p.add_argument('--bench',action='store_true')
    p.add_argument('--geometry',action='store_true')
    p.add_argument('--identifiers',action='store_true')
    p.add_argument('--durations',action='store_true')
    p.add_argument('--recording',action='store_true')
    p.add_argument('--emu',action='store_true',help='benchmark runtime Unicorn fallback, no game/shards')
    a=p.parse_args()
    repo=Path(__file__).resolve().parents[2]
    out=Path(a.out)
    (out/'src').mkdir(parents=True,exist_ok=True)
    (out/'tests').mkdir(exist_ok=True)
    source=(repo/'crates/ddlift/lift_28/src/generated/lifted_004.rs').read_text()
    match=re.search(r'pub fn f_00e0ec50\(.*?\n\}',source,re.S)
    assert match
    prelude='#![allow(warnings)]\nuse ddnative_base::{mem::Mem,x86::{self,Cpu,Flags},emit};\n'
    geometry=(repo/'crates/ddlift/lift_02/src/generated/lifted_047.rs').read_text()
    geometry_match=re.search(r'pub fn f_011787c0\(.*?\n\}',geometry,re.S)
    assert geometry_match
    identifiers=(repo/'crates/ddlift/lift_25/src/generated/lifted_033.rs').read_text()
    identifier_match=re.search(r'pub fn f_00cd8760\(.*?\n\}',identifiers,re.S)
    assert identifier_match
    duration_path=Path('E:/ddda-re/notes/native-L130/durations.rs')
    if not duration_path.exists():
        raise SystemExit('record --durations first: isolated lifted bodies required')
    duration_source=duration_path.read_text()
    recording=(repo/'crates/ddlift/lift_31/src/generated/lifted_036.rs').read_text()
    recording_match=re.search(r'pub fn f_00fedb80\(.*?\n\}',recording,re.S)
    assert recording_match
    recording_source=recording_match.group().replace('x86::call_local(m, cpu, 0x012EFEA0, crate::platform::memcpy)', 'x86::call_ext(m, cpu, 0x012EFEA0)')
    (out/'src/lib.rs').write_text(prelude+recording_source+'\n'+match.group()+'\n'+geometry_match.group()+'\n'+identifier_match.group()+'\n'+duration_source)
    deps='\n'.join(f'{n}={{path="{(repo/"crates"/n).as_posix()}"}}' for n in ('mtcore','ddnative-base','ddnative-host'))
    if a.emu:
        deps+='\nddruntime={path="'+(repo/'crates/ddruntime').as_posix()+'",default-features=false,features=["emu"]}'
    (out/'Cargo.toml').write_text('[package]\nname="main-thread-check"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n'+deps+'\nserde_json="1"\n[profile.release]\nopt-level=1\n[profile.release.package.mtcore]\nopt-level=3\n[profile.release.package.ddnative-base]\nopt-level=3\n[profile.release.package.ddnative-host]\nopt-level=3\n')
    if a.emu:
        with (out/'Cargo.toml').open('a') as manifest:
            manifest.write('[profile.release.package.ddruntime]\nopt-level=3\n')
    test=(repo/'crates/mtcore/tests/hot.rs').read_text().replace('// L130_REFERENCE_RUN','run(main_thread_check::f_00e0ec50,case);')
    test,count=re.subn(r'vec!\[\("port",\s*mtcore::hot::expand_dependencies\)\]; // L130_COST_BODIES', 'vec![("lifted",main_thread_check::f_00e0ec50),("port",mtcore::hot::expand_dependencies)];', test)
    assert count==1, 'cost-body insertion marker missing'
    test=test.replace('// L130_GEOMETRY_REFERENCE','run_with_callees(main_thread_check::f_011787c0,case);')
    test,count=re.subn(r'vec!\[\("port",\s*mtcore::hot::dispatch_geometry\)\]; // L130_GEOMETRY_COST', 'vec![("lifted",main_thread_check::f_011787c0),("port",mtcore::hot::dispatch_geometry)];',test)
    assert count==1,'geometry cost marker missing'
    test=test.replace('// L130_IDENTIFIER_REFERENCE','run_with_callees(main_thread_check::f_00cd8760,case);')
    test,count=re.subn(r'vec!\[\("port",\s*mtcore::hot::resolve_identifier\)\]; // L130_IDENTIFIER_COST', 'vec![("lifted",main_thread_check::f_00cd8760),("port",mtcore::hot::resolve_identifier)];',test)
    assert count==1,'identifier cost marker missing'
    if duration_source:
        entries=range(0x112bb40,0x112bc00,0x20)
        arms='\n'.join(f'0x{entry:x} => main_thread_check::f_{entry:08x},' for entry in entries)
        test+='\nfn duration_lifted(entry:u32)->LiftFn { match entry {\n'+arms+'\n_ => panic!("unknown duration") } }\n'
        test=test.replace('// L130_DURATION_REFERENCE','run_with_callees(duration_lifted(n(&case["entry"])),case);')
        test,count=re.subn(r'vec!\[\("port",\s*duration_port\(entry\)\)\]; // L130_DURATION_COST', 'vec![("lifted",duration_lifted(entry)),("port",duration_port(entry))];',test)
        assert count==1,'duration cost marker missing'
    elif a.durations:
        raise SystemExit('record --durations first: isolated lifted bodies required')
    test=test.replace('// L130_RECORDING_REFERENCE','run_with_callees(main_thread_check::f_00fedb80,case);')
    test,count=re.subn(r'vec!\[\("port",\s*mtcore::hot::save_draw_recording_state\)\]; // L130_RECORDING_COST', 'vec![("lifted",main_thread_check::f_00fedb80),("port",mtcore::hot::save_draw_recording_state)];',test)
    assert count==1,'recording cost marker missing'
    if a.emu: test+=EMU_TEST
    (out/'tests/hot.rs').write_text(test)
    env=dict(os.environ,CARGO_TARGET_DIR='C:/ddda-targets/L130',CARGO_BUILD_JOBS='4',CARGO_INCREMENTAL='0',DDNATIVE_REQUIRE_VECTORS='1')
    command=['cargo','test','--manifest-path',str(out/'Cargo.toml'),'--test','hot']
    benchmark='recording_cost' if a.recording else 'durations_cost' if a.durations else 'identifiers_cost' if a.identifiers else 'geometry_cost' if a.geometry else 'dependencies_cost'
    if a.emu: benchmark='durations_runtime_cost'
    command+=['--release',benchmark,'--','--ignored','--nocapture'] if a.bench else ['--','--nocapture']
    raise SystemExit(subprocess.call(command,env=env))


if __name__=='__main__': main()
