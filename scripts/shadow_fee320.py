"""Validate L162e private real-call transcripts or report timer-only cost.

Never runs the game. --replay compiles ONLY the extracted FEE320 body,
under the private output directory, and replays its external-call contracts.
Raw generated code and guest data never enter the repository.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def validate(sample):
    if sample['version'] != 1 or sample['entry'] != 0xFEE320 or sample['origin'] != 'runtime-lift':
        raise ValueError('wrong schema/entry')
    if sample['panicked'] or sample['overflow']:
        raise ValueError('incomplete shadow callback')
    if not 0 <= sample['boundary_ns'] <= sample['leaf_ns'] <= sample['shadow_ns']:
        raise ValueError('invalid nested timing')
    records = sample['records']
    if bool(records) != sample['capture'] and sample['calls']:
        raise ValueError('capture mode/record mismatch')
    if sample['capture'] and len(records) != sample['calls']:
        raise ValueError('missing FEE320 call records')
    for call in records:
        if call['overflow'] or call['panicked']:
            raise ValueError('incomplete FEE320 call')
        for state in (call['input'], call['output']):
            cpu(state)
        stack = []
        for event in call['events']:
            kind, address, *rest = event
            if not isinstance(address, int) or not 0 <= address <= 0xFFFFFFFF:
                raise ValueError('invalid address')
            if kind in ('enter', 'leave'):
                cpu(rest[0])
                if kind == 'enter':
                    stack.append(address)
                elif not stack or stack.pop() != address:
                    raise ValueError('unbalanced call boundary')
            elif kind in ('r', 'w'):
                if len(rest) != 1 or any(type(b) is not int or not 0 <= b <= 255 for b in rest[0]):
                    raise ValueError('invalid memory bytes')
            elif kind == 'probe':
                if len(rest) != 3 or type(rest[1]) is not bool:
                    raise ValueError('invalid probe')
            elif kind != 'alloc':
                raise ValueError('unknown event')
        if stack:
            raise ValueError('unterminated callee')
    return sample


def cpu(state):
    if len(state) != 84 or any(type(v) is not int or not 0 <= v <= 0xFFFFFFFF for v in state):
        raise ValueError('invalid CPU snapshot')


def summarize(samples, first, frames):
    if not samples:
        raise ValueError('no samples')
    mode = samples[0]['capture']
    if any(s['capture'] != mode for s in samples):
        raise ValueError('capture and timing runs must be separate')
    ids = [s['frame'] for s in samples]
    if sorted(ids) != list(range(first, first + frames)):
        raise ValueError('expected exactly one sky-shadow callback per requested frame')
    if len({s['object'] for s in samples}) != 1:
        raise ValueError('mixed shadow objects')
    for s in samples:
        validate(s)
    result = dict(frames=frames, calls=sum(s['calls'] for s in samples), capture=mode)
    if not result['calls']:
        raise ValueError('no FEE320 calls')
    if mode:
        result['timing_usable'] = False
        result['events'] = sum(len(c['events']) for s in samples for c in s['records'])
        result['scope'] = 'whole lifted caller; external callees retain recorded opaque contracts'
    else:
        shadow = sum(s['shadow_ns'] for s in samples)
        leaf = sum(s['leaf_ns'] for s in samples)
        boundary = sum(s['boundary_ns'] for s in samples)
        if not shadow or not result['calls']:
            raise ValueError('no measured FEE320 work')
        result.update(shadow_ms_per_frame=shadow/frames/1e6,
                      fee320_inclusive_ms_per_frame=leaf/frames/1e6,
                      fee320_external_ms_per_frame=boundary/frames/1e6,
                      fee320_excluding_external_ms_per_frame=(leaf-boundary)/frames/1e6,
                      inclusive_share=leaf/shadow,
                      excluding_external_share=(leaf-boundary)/shadow,
                      scope='instrumented wall time; includes timer/wrapper overhead and host scheduling')
    return result


HARNESS = r'''
use ddnative_base::{mem::Mem,x86::{self,Cpu}};
use serde_json::Value;
use std::{cell::RefCell,rc::Rc};
pub use ddnative_base::{platform,mem};
mod threads { pub fn current_tid()->u32 { 0 } }
#[path="@MODULE@"] mod shadow_fee320;
mod lifted {
#![allow(unused_parens)]
use ddnative_base::{mem::Mem,emit,x86::{self,Cpu,Flags}};
@BODY@
}
fn words(v:&Value)->Vec<u32> { v.as_array().unwrap().iter().map(|v|v.as_u64().unwrap().try_into().unwrap()).collect() }
fn compare_state(actual:Vec<u32>, expected:&Value) {
    let mut actual=actual; let mut expected=words(expected);
    // X87::clone contains HOST FIP/FDP addresses from hardware FNSAVE.
    // They change with ASLR/stack locations between game and replay.
    // Guest FNSAVE gets original instruction-supplied FIP/FDP instead
    // (ddnative-base/src/x87.rs::fnsave). Keep raw capture bytes, compare
    // every guest-observable field, including control/status/tags/opcode.
    for i in [60,62] {actual[i]=0;expected[i]=0;}
    assert_eq!(actual,expected,"guest CPU state (host x87 FIP/FDP excluded)");
}
fn restore(v:&Value,c:&mut Cpu) {
    let v=words(v); assert_eq!(v.len(),84);
    c.eax=v[0];c.ecx=v[1];c.edx=v[2];c.ebx=v[3];c.esp=v[4];c.ebp=v[5];c.esi=v[6];c.edi=v[7];
    for i in 0..32 { c.xmm[i/4][i%4]=v[8+i]; }
    c.fl=x86::Flags { kind:v[40] as u8,a:v[41],b:v[42],r:v[43],bits:v[44],cf_in:v[45]!=0 };
    c.df=v[46]!=0;c.fs_base=v[47];c.sys_flags=v[48];c.mxcsr=x86::Mxcsr(v[49]);
    c.seg=x86::Segments {es:v[51] as u16,cs:v[52] as u16,ss:v[53] as u16,ds:v[54] as u16,fs:v[55] as u16,gs:v[56] as u16};
    ddnative_base::x87::park();
    c.st=Default::default();
    for i in 0..27 { c.st.state[i*4..i*4+4].copy_from_slice(&v[57+i].to_le_bytes()); }
    x86::ldmxcsr(v[50]);
}
struct Tape { events:Vec<Value>, next:usize }
impl Tape {
    fn take(&mut self,kind:&str,a:u32)->Value {
        let e=self.events.get(self.next).expect("unexpected extra access").clone();
        self.next+=1; assert_eq!(e[0],kind,"event {}",self.next-1);
        assert_eq!(e[1].as_u64().unwrap(),a as u64,"address at {}",self.next-1); e
    }
}
struct Replay(Rc<RefCell<Tape>>);
impl Replay {
    fn read(&self,a:u32,n:usize)->Vec<u8> {
        let e=self.0.borrow_mut().take("r",a);
        let bytes=e[2].as_array().unwrap(); assert_eq!(bytes.len(),n);
        bytes.iter().map(|b|b.as_u64().unwrap() as u8).collect()
    }
    fn write(&self,a:u32,b:&[u8]) {
        let e=self.0.borrow_mut().take("w",a);
        assert_eq!(e[2],serde_json::json!(b),"written bytes");
    }
}
impl Mem for Replay {
    fn u8(&self,a:u32)->u8 {self.read(a,1)[0]}
    fn u16(&self,a:u32)->u16 {u16::from_le_bytes(self.read(a,2).try_into().unwrap())}
    fn u32(&self,a:u32)->u32 {u32::from_le_bytes(self.read(a,4).try_into().unwrap())}
    fn read_bytes(&self,a:u32,b:&mut[u8]) {b.copy_from_slice(&self.read(a,b.len()));}
    fn set_u8(&mut self,a:u32,v:u8) {self.write(a,&[v]);}
    fn set_u16(&mut self,a:u32,v:u16) {self.write(a,&v.to_le_bytes());}
    fn set_u32(&mut self,a:u32,v:u32) {self.write(a,&v.to_le_bytes());}
    fn probe(&self,a:u32,n:u32,w:bool)->Result<(),u32> {
        let e=self.0.borrow_mut().take("probe",a);assert_eq!(e[2],n);assert_eq!(e[3],w);
        e[4].as_u64().map_or(Ok(()),|a|Err(a as u32))
    }
    fn alloc(&mut self,n:u32)->u32 {self.0.borrow_mut().take("alloc",n)[2].as_u64().unwrap() as u32}
}
fn replay(call:&Value) {
    let tape=Rc::new(RefCell::new(Tape {events:call["events"].as_array().unwrap().clone(),next:0}));
    let external=tape.clone();
    x86::set_ext_calls(Some(Box::new(move |_,c,target| {
        let state=shadow_fee320::snapshot(c);
        let mut tape=external.borrow_mut();
        let entry=tape.take("enter",target);compare_state(state,&entry[2]);
        // Explicit opaque callee contract. We do not claim to replay its
        // implementation or service effects outside the supplied Mem.
        let mut stack=vec![target];
        while !stack.is_empty() {
            let e=tape.events.get(tape.next).expect("unterminated external call").clone(); tape.next+=1;
            match e[0].as_str().unwrap() {
                "enter"=>stack.push(e[1].as_u64().unwrap() as u32),
                "leave"=>{ assert_eq!(Some(e[1].as_u64().unwrap() as u32),stack.pop());
                    if stack.is_empty() {restore(&e[2],c);} },
                _=>{}
            }
        }
    })));
    let mut c=Cpu::default(); restore(&call["input"],&mut c);
    lifted::f_00fee320(&mut Replay(tape.clone()),&mut c);
    compare_state(shadow_fee320::snapshot(&c),&call["output"]);
    assert_eq!(tape.borrow().next,tape.borrow().events.len(),"unconsumed events");
    x86::set_ext_calls(None);
}
#[test]
fn whole_lift_real_call_oracle() {
    ddnative_host::install();
    ddnative_base::platform::redirect(&[0x12efea0]);
    let manifest:Value=serde_json::from_slice(&std::fs::read("@MANIFEST@").unwrap()).unwrap();
    let saved=x86::stmxcsr(); let mut count=0;
    for path in manifest.as_array().unwrap() {
        let sample:Value=serde_json::from_slice(&std::fs::read(path.as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(sample["origin"],"runtime-lift");
        assert_eq!(sample["capture"],true); assert_eq!(sample["overflow"],false);assert_eq!(sample["panicked"],false);
        for call in sample["records"].as_array().unwrap() {
            assert_eq!(call["overflow"],false); assert_eq!(call["panicked"],false);
            replay(call); count+=1;
        }
    }
    x86::ldmxcsr(saved);assert!(count>0,"no real calls");eprintln!("FEE320 whole-lift replay {count} calls");
}

#[test]
fn synthetic_capture_replays_whole_lift_and_detects_changes() {
    ddnative_host::install();
    struct Memory(std::collections::BTreeMap<u32,u8>);
    impl Mem for Memory {
        fn u8(&self,a:u32)->u8 {*self.0.get(&a).unwrap_or(&0)}
        fn set_u8(&mut self,a:u32,b:u8) {self.0.insert(a,b);}
    }
    for variant in 0..128 {
        let mut m=Memory(Default::default());
        let mut c=Cpu::default();c.ecx=0x10000;c.esp=0x8000;
        m.set_u32(0x8000,0xdeadbeef);m.set_u32(0x8004,0x50000);
        m.set_u32(0x1019c,0x40000);m.set_u32(0x10194,0x41000);
        m.set_u32(0x41008,0x42000);m.set_u32(0x41010,0x43000);
        m.set_u32(0x10020,0x44000);
        if variant&1!=0 {m.set_u32(0x10154,1);}
        if variant&2!=0 {m.set_u32(0x101bc,1);m.set_u32(0x18d12cc,0x60000);
            m.set_u32(0x60044,0x61000);m.set_u8(0x61024,1);}
        if variant&4!=0 {m.set_u32(0x10150,1);m.set_u32(0x10024,0x70000);m.set_u32(0x10028,0x71000);}
        if variant&8!=0 {m.set_u32(0x100c8,8);m.set_u32(0x10024,0x70000);m.set_u32(0x10028,0x71000);}
        if variant&16!=0 {m.set_u32(0x10150,1);m.set_u32(0x10024,0);m.set_u32(0x10028,0);}
        if variant&32!=0 {m.set_u32(0x10154,3);}
        if variant&64!=0 {
            m.set_u32(0x10154,0x100001);m.set_u32(0x10cc8,0x72000);m.set_u32(0x72000,1.0f32.to_bits());
            m.set_u32(0x10018,0x73000);m.set_u32(0x73568,0x74000);m.set_u16(0x74018,8);
            m.set_u32(0x74024,0x75000);m.set_u32(0x1017c,1.25f32.to_bits());
            m.set_u32(0x10024,0x70000);m.set_u32(0x10028,0x71000);
        }
        // Distinct input registers/stack/XMM, including preserved high lanes.
        c.eax=variant*0x10203;c.edx=variant^0xff;c.ebx=variant*7;
        c.xmm[7]=[variant,variant+1,variant+2,variant+3];
        // Explicit synthetic callee contracts, NOT original callee proof.
        x86::set_ext_calls(Some(Box::new(|m,c,target| {
            match target {
                0xda7d70=>{assert_eq!(m.u32(c.esp+4),0x1000);c.eax=0x70000;c.esp+=8;},
                0xe132b0=>{c.eax=0;c.esp+=32;},
                0x12efea0=>{platform::redirect(&[]);platform::memcpy(m,c);platform::redirect(&[0x12efea0]);},
                _=>panic!("unexpected synthetic callee {target:x}")
            }
        })));
        let text=shadow_fee320::test_capture(lifted::f_00fee320,&mut m,&mut c);
        let sample:Value=serde_json::from_str(&text).unwrap();
        replay(&sample["records"][0]);
        let mut corrupt=sample["records"][0].clone();
        let words=corrupt["output"].as_array_mut().unwrap();words[0]=serde_json::json!(words[0].as_u64().unwrap()^0x100);
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(||replay(&corrupt))).is_err());
    }
    x86::set_ext_calls(None);
}
'''


def prepare_replay(paths, out):
    repo = Path(__file__).resolve().parents[1]
    source = (repo/'crates/ddlift/lift_31/src/generated/lifted_036.rs').read_text()
    start = source.index('pub fn f_00fee320(')
    end = source.index('\n// Translated from ', start)
    body = source[start:end]
    out = out.resolve()
    # Refuse generating private lift/corpus artifacts inside ANY repo tree.
    if any((p/'.git').exists() for p in [out, *out.parents]):
        raise ValueError('isolated output must be outside a git worktree')
    out.mkdir(parents=True, exist_ok=True)
    manifest = out/'corpus.json'
    manifest.write_text(json.dumps([p.resolve().as_posix() for p in paths]))
    (out/'Cargo.toml').write_text(f'''[package]
name="l162e-fee320-oracle"
version="0.1.0"
edition="2021"
[workspace]
[features]
game=[]
[dependencies]
ddnative-base={{path="{repo.as_posix()}/crates/ddnative-base"}}
ddnative-host={{path="{repo.as_posix()}/crates/ddnative-host"}}
serde_json="1"
[profile.release]
opt-level=1
[profile.release.package.ddnative-base]
opt-level=3
[profile.release.package.ddnative-host]
opt-level=3
[[test]]
name="oracle"
path="oracle.rs"
''')
    text = HARNESS.replace('@MODULE@',(repo/'crates/ddruntime/src/shadow_fee320.rs').as_posix())
    text = text.replace('@BODY@',body).replace('@MANIFEST@',manifest.as_posix())
    (out/'oracle.rs').write_text(text)
    return hashlib.sha256(body.encode()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('directory', type=Path)
    p.add_argument('--first', required=True, type=int)
    p.add_argument('--frames', required=True, type=int)
    p.add_argument('--replay', action='store_true')
    p.add_argument('--out', type=Path, default=Path('E:/ddda-re/notes/native-L162e/isolated'))
    args = p.parse_args()
    paths = sorted(args.directory.glob('fee320-*.json'))
    result = summarize([json.loads(path.read_text()) for path in paths], args.first, args.frames)
    result['sources'] = {str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}
    if args.replay:
        if not result['capture']:
            raise ValueError('replay requires capture mode')
        result['lift_sha256'] = prepare_replay(paths,args.out)
        env = dict(os.environ,CARGO_INCREMENTAL='0',CARGO_BUILD_JOBS='4',CARGO_TARGET_DIR='C:/ddda-targets/L162')
        subprocess.run(['cargo','test','--manifest-path',str(args.out/'Cargo.toml'),'--release',
                        'whole_lift_real_call_oracle','--','--nocapture','--test-threads=1'],env=env,check=True)
    print(json.dumps(result,indent=2))


if __name__ == '__main__':
    main()
