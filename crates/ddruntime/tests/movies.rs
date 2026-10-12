//! WMVCore COM ABI tests. Installed-media tests also traverse original
//! delay-load stubs. The optional original-IStream probe diagnoses missing lifts.
use ddnative_base::{mem::Mem, x86::Cpu};
use ddruntime::{
    dispatch::Registry,
    interfaces,
    memory::{GuestMemory, READ, WRITE},
    movies::{self, Decoder, GuestStream, Kind, MediaType, Reply, Request, Sample},
    runtime, services,
};
use std::sync::{Arc, Mutex};
const DATA: u32 = 0x10000;
const STACK: u32 = 0x10010000;
fn setup() -> (GuestMemory, Registry) {
    runtime::enter(Arc::new(runtime::Runtime::default()));
    let mut m = GuestMemory::new();
    m.map(DATA, 0x10000, READ | WRITE).unwrap();
    m.map(STACK - 0x10000, 0x10000, READ | WRITE).unwrap();
    let mut r = Registry::new();
    services::install(&mut m, &mut r).unwrap();
    movies::install(&mut m, &mut r, &mut interfaces::Tokens::default()).unwrap();
    (m, r)
}
fn api(m: &mut dyn Mem, f: fn(&mut dyn Mem, &mut Cpu), a: &[u32]) -> u32 {
    let mut c = frame(m, a);
    let end = c.esp + 4 + 4 * a.len() as u32;
    f(m, &mut c);
    assert_eq!(c.esp, end);
    c.eax
}
fn frame(m: &mut dyn Mem, a: &[u32]) -> Cpu {
    let esp = STACK - 0x100;
    m.set_u32(esp, 0xdeadbeef);
    for (i, &v) in a.iter().enumerate() {
        m.set_u32(esp + 4 + 4 * i as u32, v);
    }
    Cpu {
        esp,
        ..Cpu::default()
    }
}
fn method(m: &mut dyn Mem, r: &Registry, p: u32, slot: u32, a: &[u32]) -> u32 {
    let mut args = vec![p];
    args.extend(a);
    let f = m.u32(m.u32(p) + 4 * slot);
    let mut c = frame(m, &args);
    let end = c.esp + 4 + 4 * args.len() as u32;
    r.call(m, &mut c, f).unwrap();
    assert_eq!(c.esp, end, "slot {slot}");
    c.eax
}
fn put(m: &mut dyn Mem, p: u32, b: &[u8]) {
    for (i, &v) in b.iter().enumerate() {
        m.set_u8(p + i as u32, v)
    }
}
fn guid(m: &mut dyn Mem, p: u32, id: u128) {
    let b = id.to_be_bytes();
    m.set_u32(p, u32::from_be_bytes(b[..4].try_into().unwrap()));
    m.set_u16(p + 4, u16::from_be_bytes(b[4..6].try_into().unwrap()));
    m.set_u16(p + 6, u16::from_be_bytes(b[6..8].try_into().unwrap()));
    put(m, p + 8, &b[8..]);
}
struct Fake {
    log: Arc<Mutex<Vec<Request>>>,
    samples: u32,
}
impl Decoder for Fake {
    fn request(&mut self, q: Request, _: &mut dyn GuestStream) -> movies::Result<Reply> {
        let reply = match &q {
            Request::Create(k, _) => Reply::Object(1, *k),
            Request::OutputCount(_) => Reply::Number(2),
            Request::Props(..) | Request::Format(..) => Reply::Object(2, Kind::Media),
            Request::Media(_) => Reply::Media(MediaType {
                major: [1; 16],
                subtype: [2; 16],
                fixed: 1,
                sample_size: 6,
                format_type: [3; 16],
                format: vec![4, 5, 6, 7],
                ..Default::default()
            }),
            Request::StreamNumber(..) => Reply::Number(0x1234),
            Request::Attribute(..) => Reply::Attribute(4, 0x100000002u64.to_le_bytes().to_vec()),
            Request::Next(..) => {
                self.samples += 1;
                if self.samples > 1 {
                    return Err(0xc00d0bcf);
                }
                Reply::Sample(Sample {
                    bytes: vec![1, 2, 3, 4, 5, 6],
                    time: 0x100000002,
                    duration: 0x300000004,
                    flags: 2,
                    output: 1,
                    stream: 0x1234,
                })
            }
            _ => Reply::Done,
        };
        self.log.lock().unwrap().push(q);
        Ok(reply)
    }
}
#[test]
fn media_wire_samples_identity_and_lifetimes() {
    let (mut m, r) = setup();
    let log = Arc::new(Mutex::new(vec![]));
    movies::set_decoder(Box::new(Fake {
        log: log.clone(),
        samples: 0,
    }));
    assert_eq!(api(&mut m, movies::create_reader, &[0, 1, DATA]), 0);
    let reader = m.u32(DATA);
    assert_eq!(method(&mut m, &r, reader, 14, &[DATA + 4]), 0);
    assert_eq!(m.u32(DATA + 4), 2);
    assert_eq!(method(&mut m, &r, reader, 15, &[0, DATA + 4]), 0);
    let props = m.u32(DATA + 4);
    assert_eq!(method(&mut m, &r, props, 4, &[0, DATA + 8]), 0);
    assert_eq!(m.u32(DATA + 8), 76);
    m.set_u32(DATA + 8, 75);
    m.set_u8(DATA + 0x100, 0xaa);
    assert_eq!(
        method(&mut m, &r, props, 4, &[DATA + 0x100, DATA + 8]),
        0xc00d07d1
    );
    assert_eq!(m.u8(DATA + 0x100), 0xaa);
    assert_eq!(method(&mut m, &r, props, 4, &[DATA + 0x100, DATA + 8]), 0);
    assert_eq!(m.u32(DATA + 0x100 + 60), 0);
    assert_eq!(m.u32(DATA + 0x100 + 64), 4);
    assert_eq!(m.u32(DATA + 0x100 + 68), DATA + 0x100 + 72);
    assert_eq!(m.u32(DATA + 0x100 + 72), 0x07060504);
    m.set_u32(DATA + 0x10, 0xffffffff);
    assert_eq!(method(&mut m, &r, reader, 20, &[0, DATA + 0x10]), 0);
    assert_eq!(m.u32(DATA + 0x10), 0xffff1234);
    guid(&mut m, DATA + 0x200, 0x96406bda_2b2b_11d3_b36b_00c04f6108ff);
    assert_eq!(method(&mut m, &r, reader, 0, &[DATA + 0x200, DATA + 4]), 0);
    let header = m.u32(DATA + 4);
    assert_ne!(header, reader);
    wide(&mut m, DATA + 0x300, "Duration");
    m.set_u16(DATA + 0x80, 0);
    assert_eq!(
        method(
            &mut m,
            &r,
            header,
            5,
            &[0, DATA + 0x300, DATA + 0x84, 0, DATA + 0x80]
        ),
        0
    );
    assert_eq!(m.u16(DATA + 0x80), 8);
    assert_eq!(m.u32(DATA + 0x84), 4); // WMT_TYPE_QWORD
    m.set_u16(DATA + 0x80, 7);
    m.set_u32(DATA + 0x88, 0xfeed1234);
    m.set_u32(DATA + 0x90, 0xaabbccdd);
    assert_eq!(
        method(
            &mut m,
            &r,
            header,
            5,
            &[DATA + 0x88, DATA + 0x300, DATA + 0x84, DATA + 0x90, DATA + 0x80]
        ),
        0xc00d07d1
    );
    assert_eq!(m.u32(DATA + 0x90), 0xaabbccdd);
    assert_eq!(
        method(
            &mut m,
            &r,
            header,
            5,
            &[DATA + 0x88, DATA + 0x300, DATA + 0x84, DATA + 0x90, DATA + 0x80]
        ),
        0
    );
    assert_eq!(read64(&m, DATA + 0x90), 0x100000002);
    assert_eq!(m.u32(DATA + 0x88), 0xfeed1234);
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .any(|q| matches!(q, Request::Attribute(_, None, _))));
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .any(|q| matches!(q, Request::Attribute(_, Some(0x1234), _))));
    guid(&mut m, DATA + 0x200, 0x00000000_0000_0000_c000_000000000046);
    assert_eq!(method(&mut m, &r, header, 0, &[DATA + 0x200, DATA + 4]), 0);
    assert_eq!(m.u32(DATA + 4), reader);
    assert_eq!(method(&mut m, &r, header, 2, &[]), 2);
    assert_eq!(method(&mut m, &r, reader, 2, &[]), 1);
    guid(&mut m, DATA + 0x200, 1);
    assert_eq!(
        method(&mut m, &r, reader, 0, &[DATA + 0x200, DATA + 4]),
        movies::E_NOINTERFACE
    );
    assert_eq!(m.u32(DATA + 4), 0);
    assert_eq!(method(&mut m, &r, reader, 5, &[2, 1, 4, 3]), 0);
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .any(|q| matches!(q, Request::Range(_, 0x100000002, 0x300000004))));
    assert_eq!(
        method(
            &mut m,
            &r,
            reader,
            7,
            &[0, DATA + 4, DATA + 8, DATA + 16, DATA + 24, DATA + 28, DATA + 32]
        ),
        0
    );
    let sample = m.u32(DATA + 4);
    assert_eq!(read64(&m, DATA + 8), 0x100000002);
    assert_eq!(read64(&m, DATA + 16), 0x300000004);
    assert_eq!(m.u32(DATA + 24), 2);
    assert_eq!(m.u32(DATA + 28), 1);
    assert_eq!(m.u16(DATA + 32), 0x1234);
    assert_eq!(method(&mut m, &r, sample, 7, &[DATA + 40, DATA + 44]), 0);
    let data = m.u32(DATA + 40);
    assert_eq!(m.u32(DATA + 44), 6);
    assert_eq!(m.u8(data + 5), 6);
    assert_eq!(method(&mut m, &r, sample, 4, &[7]), movies::E_INVALIDARG);
    assert_eq!(method(&mut m, &r, sample, 4, &[3]), 0);
    assert_eq!(method(&mut m, &r, sample, 3, &[DATA + 44]), 0);
    assert_eq!(m.u32(DATA + 44), 3);
    assert_eq!(method(&mut m, &r, sample, 2, &[]), 0);
    assert_eq!(services::heap_size_of(data), None);
    assert_eq!(services::heap_size_of(sample), None);
    assert_eq!(
        method(&mut m, &r, reader, 7, &[0, DATA + 4, 0, 0, 0, 0, 0]),
        0xc00d0bcf
    );
    assert_eq!(m.u32(DATA + 4), 0);
    assert_eq!(method(&mut m, &r, props, 2, &[]), 0);
    assert_eq!(method(&mut m, &r, reader, 2, &[]), 0);
    assert_eq!(services::heap_size_of(header), None);
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .any(|q| matches!(q, Request::Release(1))));
}
#[test]
fn all_delay_exports_resolve_through_win32() {
    let (mut m, r) = setup();
    let log = Arc::new(Mutex::new(vec![]));
    movies::set_decoder(Box::new(Fake { log, samples: 0 }));
    put(&mut m, DATA, b"WMVCore.DLL\0");
    let module = api(&mut m, ddruntime::win32::load_library_a, &[DATA]);
    assert_ne!(module, 0);
    for (name, args) in [
        ("WMCreateSyncReader", vec![0, 1, DATA + 0x200]),
        ("WMCreateWriter", vec![0, DATA + 0x200]),
        ("WMCreateProfileManager", vec![DATA + 0x200]),
    ] {
        put(&mut m, DATA + 0x100, format!("{name}\0").as_bytes());
        let token = api(
            &mut m,
            ddruntime::win32::get_proc_address,
            &[module, DATA + 0x100],
        );
        assert_ne!(token, 0);
        let mut c = frame(&mut m, &args);
        let end = c.esp + 4 + 4 * args.len() as u32;
        r.call(&mut m, &mut c, token).unwrap();
        assert_eq!(c.eax, 0);
        assert_eq!(c.esp, end);
        let p = m.u32(DATA + 0x200);
        assert_ne!(p, 0);
        assert_eq!(method(&mut m, &r, p, 2, &[]), 0);
    }
}

struct BridgeProbe;
impl Decoder for BridgeProbe {
    fn request(&mut self, q: Request, guest: &mut dyn GuestStream) -> movies::Result<Reply> {
        match q {
            Request::Create(k, _) => Ok(Reply::Object(1, k)),
            Request::Open(_, stream) => {
                let stat = guest.call(stream, movies::StreamOp::Stat(1));
                if stat.value == 0 {
                    return Err(0xc00d0026); // NS_E_UNRECOGNIZED_STREAM_TYPE
                }
                assert_eq!((stat.hr, stat.value), (0, 6));
                let first = guest.call(stream, movies::StreamOp::Read(4));
                assert_eq!((first.hr, first.bytes), (0, vec![10, 20, 30, 40]));
                let seek = guest.call(stream, movies::StreamOp::Seek(-2, 1));
                assert_eq!((seek.hr, seek.value), (0, 2));
                let tail = guest.call(stream, movies::StreamOp::Read(8));
                assert_eq!((tail.hr, tail.bytes), (0, vec![30, 40, 50, 60]));
                let eof = guest.call(stream, movies::StreamOp::Read(1));
                assert_eq!((eof.hr, eof.value, eof.bytes.len()), (1, 0, 0));
                Ok(Reply::Done)
            }
            _ => Ok(Reply::Done),
        }
    }
}

#[test]
fn stream_callbacks_own_a_reference_and_preserve_short_reads_and_seek64() {
    use ddruntime::dispatch::Binding;
    let (mut m, mut r) = setup();
    movies::set_decoder(Box::new(BridgeProbe));
    for (slot, f) in [
        (1, fixture_addref as fn(&mut dyn Mem, &mut Cpu)),
        (2, fixture_release),
        (3, fixture_read),
        (5, fixture_seek),
        (12, fixture_stat),
    ] {
        let token = 0x1000 + slot * 16;
        r.insert(token, Binding::Service(f)).unwrap();
        m.set_u32(DATA + 0x400 + slot * 4, token);
    }
    let r = Arc::new(r);
    r.install();
    let stream = DATA + 0x100;
    m.set_u32(stream, DATA + 0x400);
    m.set_u32(stream + 4, 1);
    m.set_u32(stream + 8, DATA + 0x200);
    m.set_u32(stream + 12, 6);
    put(&mut m, DATA + 0x200, &[10, 20, 30, 40, 50, 60]);
    assert_eq!(api(&mut m, movies::create_reader, &[0, 1, DATA]), 0);
    let reader = m.u32(DATA);
    assert_eq!(method(&mut m, &r, reader, 23, &[stream]), 0);
    assert_eq!(m.u32(stream + 4), 2);
    m.set_u32(stream + 16, 0);
    assert_eq!(method(&mut m, &r, reader, 23, &[stream]), 0);
    assert_eq!(m.u32(stream + 4), 2);
    let bad_stream = DATA + 0x180;
    m.set_u32(bad_stream, DATA + 0x400);
    m.set_u32(bad_stream + 4, 1);
    assert_eq!(method(&mut m, &r, reader, 23, &[bad_stream]), 0xc00d0026);
    assert_eq!(m.u32(stream + 4), 1);
    assert_eq!(m.u32(bad_stream + 4), 1);
    m.set_u32(stream + 16, 0);
    assert_eq!(method(&mut m, &r, reader, 23, &[stream]), 0);
    assert_eq!(method(&mut m, &r, reader, 4, &[]), 0);
    assert_eq!(m.u32(stream + 4), 1);
    m.set_u32(stream + 16, 0);
    assert_eq!(method(&mut m, &r, reader, 23, &[stream]), 0);
    assert_eq!(method(&mut m, &r, reader, 2, &[]), 0);
    assert_eq!(m.u32(stream + 4), 1);
    assert_eq!(m.u32(stream + 20), 12);
    assert_eq!(m.u32(stream + 24), 4);
    assert_eq!(m.u32(stream + 28), 4);
}
fn fixture_addref(m: &mut dyn Mem, c: &mut Cpu) {
    fixture_stream(m, c, 1)
}
fn fixture_release(m: &mut dyn Mem, c: &mut Cpu) {
    fixture_stream(m, c, 2)
}
fn fixture_read(m: &mut dyn Mem, c: &mut Cpu) {
    fixture_stream(m, c, 3)
}
fn fixture_seek(m: &mut dyn Mem, c: &mut Cpu) {
    fixture_stream(m, c, 5)
}
fn fixture_stat(m: &mut dyn Mem, c: &mut Cpu) {
    fixture_stream(m, c, 12)
}

#[cfg(all(windows, feature = "game"))]
#[test]
#[ignore = "requires DDDA_EXE, DDDA_MOVIE and installed generated native program; real system WMVCore"]
fn installed_movie_delay_and_guest_stream() {
    installed(false);
}

#[cfg(all(windows, feature = "game"))]
#[test]
#[ignore = "requires the original IStream AddRef/Release/Seek lifts, DDDA_EXE and DDDA_MOVIE"]
fn installed_movie_original_stream() {
    installed(true);
}

#[cfg(all(windows, feature = "game"))]
fn installed(original: bool) {
    use ddruntime::session::{Outcome, Session};
    let mut s = Session::new(&std::env::var("DDDA_EXE").expect("DDDA_EXE")).unwrap();
    if original {
        let missing: Vec<_> = [0x00f7b650, 0x00f7b660, 0x00f7b7d0]
            .into_iter()
            .filter(|a| s.registry.get(*a).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "original IStream needs native entries: {missing:08X?}"
        );
    }
    s.mem.map(DATA, 0x10000, READ | WRITE).unwrap();
    s.mem.map(STACK - 0x10000, 0x10000, READ | WRITE).unwrap();
    let movie = std::fs::read(std::env::var("DDDA_MOVIE").expect("DDDA_MOVIE")).unwrap();
    let storage = api(
        &mut s.mem,
        ddruntime::win32::heap_alloc,
        &[ddruntime::win32::PROCESS_HEAP, 8, movie.len() as u32],
    );
    put(&mut s.mem, storage, &movie);
    let stream = api(
        &mut s.mem,
        ddruntime::win32::heap_alloc,
        &[ddruntime::win32::PROCESS_HEAP, 8, 0x150],
    );
    if original {
        // 0x00F7DFE0 constructs the actual guest IStream; 0x00F7E090's
        // memory branch fills these fields. No original body is replaced.
        let result = s.call(0x00f7dfe0, stream, &[]);
        assert!(matches!(&result, Outcome::Returned(_)), "{result}");
        s.mem.set_u32(stream + 0x144, storage);
        s.mem.set_u32(stream + 0x148, movie.len() as u32);
        s.mem.set_u32(stream + 0x14c, 0);
    } else {
        s.mem.set_u32(stream, DATA + 0x400);
        s.mem.set_u32(stream + 4, 1);
        s.mem.set_u32(stream + 8, storage);
        s.mem.set_u32(stream + 12, movie.len() as u32);
        for slot in [1, 2, 3, 5, 12] {
            s.mem.set_u32(DATA + 0x400 + slot * 4, 0x1000 + slot * 16);
        }
        let registry = s.registry.clone();
        ddnative_base::x86::set_ext_calls(Some(Box::new(move |m, c, target| {
            if (0x1000..0x1100).contains(&target) {
                fixture_stream(m, c, (target - 0x1000) / 16)
            } else {
                registry.call(m, c, target).unwrap()
            }
        })));
    }
    // This is the original delay import thunk, including ___delayLoadHelper2.
    let result = s.call(0x012ed133, 0, &[0, 1, DATA]);
    assert!(matches!(&result,Outcome::Returned(c) if c.eax==0), "{result}");
    let reader = s.mem.u32(DATA);
    assert!((interfaces::METHOD_TOKENS..interfaces::METHOD_TOKENS + 0x10000).contains(&s.mem.u32(0x018765c8)));
    for (thunk, slot, args) in [
        (0x012ed143, 0x018765c0, vec![DATA + 4]),
        (0x012ed153, 0x018765c4, vec![0, DATA + 4]),
    ] {
        let result = s.call(thunk, 0, &args);
        assert!(matches!(&result,Outcome::Returned(c) if c.eax==0), "{result}");
        assert!((interfaces::METHOD_TOKENS..interfaces::METHOD_TOKENS + 0x10000).contains(&s.mem.u32(slot)));
        let object = s.mem.u32(DATA + 4);
        assert_eq!(method(&mut s.mem, &s.registry, object, 2, &[]), 0);
    }
    s.mem.set_u32(DATA + 0x2000 + 0x1f8, reader);
    assert_eq!(method(&mut s.mem, &s.registry, reader, 23, &[stream]), 0);
    assert_eq!(s.mem.u32(stream + 4), 2, "reader owns guest stream");
    assert_eq!(method(&mut s.mem, &s.registry, reader, 14, &[DATA + 4]), 0);
    let outputs = s.mem.u32(DATA + 4);
    assert!(outputs > 0);
    let mut video = None;
    for output in 0..outputs {
        assert_eq!(
            method(&mut s.mem, &s.registry, reader, 17, &[output, DATA + 4]),
            0
        );
        let count = s.mem.u32(DATA + 4);
        for format in 0..count {
            assert_eq!(
                method(&mut s.mem, &s.registry, reader, 18, &[output, format, DATA + 8]),
                0
            );
            let props = s.mem.u32(DATA + 8);
            assert_eq!(method(&mut s.mem, &s.registry, props, 4, &[0, DATA + 12]), 0);
            assert!(s.mem.u32(DATA + 12) < 0x1000);
            assert_eq!(
                method(&mut s.mem, &s.registry, props, 4, &[DATA + 0x1000, DATA + 12]),
                0
            );
            if s.mem.u32(DATA + 0x1010) == 0x32315659 {
                // YV12 IID, exe 0x014E75CC
                let fmt = s.mem.u32(DATA + 0x1044);
                let width = s.mem.u32(fmt + 52);
                let height = s.mem.u32(fmt + 56);
                let selected = s.call(0x00f7b250, DATA + 0x2000, &[output, format, props, DATA + 0x1000]);
                assert!(
                    matches!(&selected,Outcome::Returned(c) if c.eax&255==1),
                    "{selected}"
                );
                assert_eq!(s.mem.u32(DATA + 0x2010), width);
                assert_eq!(s.mem.u32(DATA + 0x2014), height);
                assert_eq!(
                    method(&mut s.mem, &s.registry, reader, 20, &[output, DATA + 16]),
                    0
                );
                video = Some((s.mem.u16(DATA + 16), width, height));
            }
            assert_eq!(method(&mut s.mem, &s.registry, props, 2, &[]), 0);
            if video.is_some() {
                break;
            }
        }
        if video.is_some() {
            break;
        }
    }
    let (stream_number, width, height) = video.expect("system offers YV12");
    assert!(width > 0 && height > 0);
    let mut audio = None;
    let mut audio_outputs = 0;
    for output in 0..outputs {
        assert_eq!(
            method(&mut s.mem, &s.registry, reader, 15, &[output, DATA + 8]),
            0
        );
        let props = s.mem.u32(DATA + 8);
        let media = get_media(&mut s.mem, &s.registry, props);
        let is_audio = s.mem.u32(media) == 0x73647561;
        assert_eq!(method(&mut s.mem, &s.registry, props, 2, &[]), 0);
        if !is_audio {
            continue;
        }
        audio_outputs += 1;
        for (name, ty, value) in [("EnableDiscreteOutput", 3, 1), ("SpeakerConfig", 0, 6)] {
            wide(&mut s.mem, DATA + 0x800, name);
            s.mem.set_u32(DATA + 64, value);
            assert_eq!(
                method(
                    &mut s.mem,
                    &s.registry,
                    reader,
                    13,
                    &[output, DATA + 0x800, ty, DATA + 64, 4]
                ),
                0
            );
        }
        assert_eq!(
            method(&mut s.mem, &s.registry, reader, 18, &[output, 0, DATA + 8]),
            0
        );
        let props = s.mem.u32(DATA + 8);
        assert_eq!(method(&mut s.mem, &s.registry, reader, 16, &[output, props]), 0);
        let media = get_media(&mut s.mem, &s.registry, props);
        let fmt = s.mem.u32(media + 68);
        let channels = s.mem.u16(fmt + 2);
        s.mem.set_u32(DATA + 0x201c, channels as u32);
        assert_eq!(method(&mut s.mem, &s.registry, props, 2, &[]), 0);
        assert_eq!(
            method(&mut s.mem, &s.registry, reader, 17, &[output, DATA + 4]),
            0
        );
        let count = s.mem.u32(DATA + 4);
        for format in 0..count {
            assert_eq!(
                method(&mut s.mem, &s.registry, reader, 18, &[output, format, DATA + 8]),
                0
            );
            let props = s.mem.u32(DATA + 8);
            let media = get_media(&mut s.mem, &s.registry, props);
            let fmt = s.mem.u32(media + 68);
            if s.mem.u32(media + 16) == 1
                && s.mem.u32(fmt + 4) == 48000
                && s.mem.u16(fmt + 14) >= 16
                && s.mem.u16(fmt + 2) == channels
            {
                let block = s.mem.u16(fmt + 12);
                let bits = s.mem.u16(fmt + 14);
                let selected = s.call(0x00f7b250, DATA + 0x2000, &[output, format, props, media]);
                assert!(
                    matches!(&selected,Outcome::Returned(c) if c.eax&255==1),
                    "{selected}"
                );
                audio = Some((s.mem.u32(DATA + 0x2000 + 0x1f0), channels, bits, block));
            }
            assert_eq!(method(&mut s.mem, &s.registry, props, 2, &[]), 0);
            if audio.is_some() {
                break;
            }
        }
    }
    assert_eq!(
        method(&mut s.mem, &s.registry, reader, 10, &[stream_number as u32, 0]),
        0
    );
    assert_eq!(method(&mut s.mem, &s.registry, reader, 5, &[0, 0, 0, 0]), 0);
    let output = s.mem.u32(DATA + 0x2000 + 0x1fc);
    wide(&mut s.mem, DATA + 0x800, "VideoSampleDurations");
    s.mem.set_u32(DATA + 64, 1);
    assert_eq!(
        method(
            &mut s.mem,
            &s.registry,
            reader,
            13,
            &[output, DATA + 0x800, 3, DATA + 64, 4]
        ),
        0
    );
    guid(&mut s.mem, DATA + 0x900, 0x96406bda_2b2b_11d3_b36b_00c04f6108ff);
    assert_eq!(
        method(&mut s.mem, &s.registry, reader, 0, &[DATA + 0x900, DATA + 68]),
        0
    );
    let header = s.mem.u32(DATA + 68);
    wide(&mut s.mem, DATA + 0x800, "Duration");
    s.mem.set_u16(DATA + 72, 8);
    let null_stream_hr = method(
        &mut s.mem,
        &s.registry,
        header,
        5,
        &[0, DATA + 0x800, DATA + 76, DATA + 80, DATA + 72],
    );
    // 0x00F7CBE0 passes NULL and tolerates failure. The installed WMVCore
    // returns E_INVALIDARG for it; do not silently substitute stream zero.
    assert!(null_stream_hr == 0 || null_stream_hr == movies::E_INVALIDARG);
    eprintln!("Duration with NULL stream: HRESULT={null_stream_hr:#010x}");
    s.mem.set_u16(DATA + 92, 0);
    s.mem.set_u16(DATA + 72, 8);
    assert_eq!(
        method(
            &mut s.mem,
            &s.registry,
            header,
            5,
            &[DATA + 92, DATA + 0x800, DATA + 76, DATA + 80, DATA + 72]
        ),
        0
    );
    assert_eq!(s.mem.u16(DATA + 72), 8);
    assert!(read64(&s.mem, DATA + 80) > 0);
    assert_eq!(method(&mut s.mem, &s.registry, header, 2, &[]), 1);
    assert_eq!(audio.is_some(), audio_outputs > 0, "48 kHz PCM selection");
    let mut first_time = None;
    let mut previous = None;
    let mut nontrivial = false;
    for frame in 0..16 {
        if frame == 8 {
            if nontrivial {
                break;
            }
            // The install's st100ev00 starts with three seconds of black.
            // Check the opening frames first, then seek past that lead-in
            // so constant black Y / neutral chroma cannot pass this test.
            assert_eq!(
                method(&mut s.mem, &s.registry, reader, 5, &[30_000_000, 0, 0, 0]),
                0
            );
        }
        assert_eq!(
            method(
                &mut s.mem,
                &s.registry,
                reader,
                7,
                &[
                    stream_number as u32,
                    DATA + 20,
                    DATA + 24,
                    DATA + 32,
                    DATA + 40,
                    DATA + 44,
                    DATA + 48
                ]
            ),
            0
        );
        let sample = s.mem.u32(DATA + 20);
        let time = read64(&s.mem, DATA + 24);
        let duration = read64(&s.mem, DATA + 32);
        assert!(duration > 0);
        if let Some(t) = previous {
            assert!(time >= t)
        }
        first_time.get_or_insert(time);
        previous = Some(time);
        assert_eq!(s.mem.u16(DATA + 48), stream_number);
        assert_eq!(
            method(&mut s.mem, &s.registry, sample, 7, &[DATA + 52, DATA + 56]),
            0
        );
        let p = s.mem.u32(DATA + 52);
        let len = s.mem.u32(DATA + 56);
        assert_eq!(len, width * height * 3 / 2);
        nontrivial |= (0..width * height)
            .step_by(101)
            .any(|i| s.mem.u8(p + i) != s.mem.u8(p));
        eprintln!("YV12 {width}x{height}, bytes={len}, time={time}, duration={duration}");
        assert_eq!(method(&mut s.mem, &s.registry, sample, 2, &[]), 0);
    }
    assert!(nontrivial, "decoded pixels vary");
    assert!(previous.unwrap() > first_time.unwrap(), "timestamps advance");
    if let Some((stream, channels, bits, block)) = audio {
        assert_eq!(method(&mut s.mem, &s.registry, reader, 10, &[stream, 0]), 0);
        assert_eq!(method(&mut s.mem, &s.registry, reader, 5, &[0, 0, 0, 0]), 0);
        assert_eq!(
            method(
                &mut s.mem,
                &s.registry,
                reader,
                7,
                &[
                    stream,
                    DATA + 20,
                    DATA + 24,
                    DATA + 32,
                    DATA + 40,
                    DATA + 44,
                    DATA + 48
                ]
            ),
            0
        );
        let sample = s.mem.u32(DATA + 20);
        assert_eq!(
            method(&mut s.mem, &s.registry, sample, 7, &[DATA + 52, DATA + 56]),
            0
        );
        let len = s.mem.u32(DATA + 56);
        assert!(len > 0);
        assert_eq!(len % block as u32, 0);
        eprintln!(
            "PCM 48000 Hz, channels={channels}, bits={bits}, bytes={len}, time={}",
            read64(&s.mem, DATA + 24)
        );
        assert_eq!(method(&mut s.mem, &s.registry, sample, 2, &[]), 0);
    }
    // 0x00F7C389 requests stream zero (interleaved), then dispatches using
    // the returned stream number rather than assuming video comes first.
    assert_eq!(method(&mut s.mem, &s.registry, reader, 5, &[0, 0, 0, 0]), 0);
    assert_eq!(
        method(
            &mut s.mem,
            &s.registry,
            reader,
            7,
            &[
                0,
                DATA + 20,
                DATA + 24,
                DATA + 32,
                DATA + 40,
                DATA + 44,
                DATA + 48
            ]
        ),
        0
    );
    let sample = s.mem.u32(DATA + 20);
    let returned_stream = s.mem.u16(DATA + 48);
    assert!(returned_stream == stream_number || audio.is_some_and(|a| a.0 == returned_stream as u32));
    assert_eq!(
        method(
            &mut s.mem,
            &s.registry,
            reader,
            19,
            &[returned_stream as u32, DATA + 60]
        ),
        0
    );
    assert_eq!(s.mem.u32(DATA + 44), s.mem.u32(DATA + 60));
    assert_eq!(method(&mut s.mem, &s.registry, sample, 2, &[]), 0);
    assert_eq!(method(&mut s.mem, &s.registry, reader, 4, &[]), 0);
    assert_eq!(s.mem.u32(stream + 4), 1);
    assert_eq!(method(&mut s.mem, &s.registry, reader, 2, &[]), 0);
    if !original {
        assert!(s.mem.u32(stream + 20) > 0);
        assert!(s.mem.u32(stream + 24) > 0);
        assert!(s.mem.u32(stream + 28) > 0);
    }
}

#[cfg(all(windows, feature = "game"))]
fn get_media(m: &mut dyn Mem, r: &Registry, props: u32) -> u32 {
    assert_eq!(method(m, r, props, 4, &[0, DATA + 12]), 0);
    assert!(m.u32(DATA + 12) < 0x1000);
    assert_eq!(method(m, r, props, 4, &[DATA + 0x1000, DATA + 12]), 0);
    DATA + 0x1000
}

fn read64(m: &dyn Mem, p: u32) -> u64 {
    m.u32(p) as u64 | ((m.u32(p + 4) as u64) << 32)
}

fn wide(m: &mut dyn Mem, p: u32, s: &str) {
    for (i, v) in s.encode_utf16().chain([0]).enumerate() {
        m.set_u16(p + i as u32 * 2, v);
    }
}
// A test IStream whose data and cursor live entirely in guest memory.
// The WMVCore adapter must reach it through call_ext on the calling thread.
fn fixture_stream(m: &mut dyn Mem, c: &mut Cpu, slot: u32) {
    let a = |m: &dyn Mem, k: u32| m.u32(c.esp + 4 + 4 * k);
    let p = a(m, 0);
    let mut hr = 0;
    let n = match slot {
        1 | 2 => {
            let count = m.u32(p + 4);
            let count = if slot == 1 { count + 1 } else { count - 1 };
            m.set_u32(p + 4, count);
            hr = count;
            1
        }
        3 => {
            let (dst, count, out) = (a(m, 1), a(m, 2), a(m, 3));
            let pos = m.u32(p + 16);
            let len = m.u32(p + 12);
            let got = count.min(len - pos);
            let data = m.u32(p + 8);
            for k in 0..got {
                m.set_u8(dst + k, m.u8(data + pos + k));
            }
            m.set_u32(out, got);
            m.set_u32(p + 16, pos + got);
            m.set_u32(p + 20, m.u32(p + 20) + 1);
            // Match DDDA's short-read convention; exhausted input differs.
            hr = if got == 0 && count != 0 { 1 } else { 0 };
            4
        }
        5 => {
            let (delta, origin, out) = (read64(m, c.esp + 8) as i64, a(m, 3), a(m, 4));
            let base = match origin {
                0 => 0,
                1 => m.u32(p + 16),
                2 => m.u32(p + 12),
                _ => panic!("origin"),
            };
            let pos = base as i64 + delta;
            assert!(pos >= 0 && pos <= m.u32(p + 12) as i64);
            m.set_u32(p + 16, pos as u32);
            if out != 0 {
                m.set_u32(out, pos as u32);
                m.set_u32(out + 4, 0)
            }
            m.set_u32(p + 24, m.u32(p + 24) + 1);
            5
        }
        12 => {
            let out = a(m, 1);
            assert_eq!(a(m, 2), 1);
            m.set_u32(out + 4, 2);
            m.set_u32(out + 8, m.u32(p + 12));
            m.set_u32(out + 12, 0);
            m.set_u32(p + 28, m.u32(p + 28) + 1);
            3
        }
        _ => panic!("unexpected IStream slot {slot}"),
    };
    c.eax = hr;
    c.esp += 4 + 4 * n;
}
