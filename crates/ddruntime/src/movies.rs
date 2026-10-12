//! WMVCore's 32-bit COM boundary. DDDA.exe 2017-12-13 Steam:
//! 0x00F7DEB0 opens the guest IStream, 0x00F7CA40/0x00F7BAE0 enumerate
//! outputs, 0x00F7B250 selects YV12/PCM, 0x00F7C200 reads samples.
//! Host codecs are behind Decoder; no host pointer enters guest memory.
use crate::{
    dispatch::{Conflict, Registry},
    interfaces::{self, Spec, Tokens},
    services::{arg, call_guest, ret, with_state},
};
use ddnative_base::{mem::Mem, x86::Cpu};
use std::collections::HashMap;

#[cfg(windows)]
mod windows;
pub const E_POINTER: u32 = 0x80004003;
pub const E_INVALIDARG: u32 = 0x80070057;
pub const E_NOTIMPL: u32 = 0x80004001;
pub const E_NOINTERFACE: u32 = 0x80004002;
pub const E_FAIL: u32 = 0x80004005;
// asferr.h; IWMMediaProps::GetMediaType documents this HRESULT.
const BUFFER_TOO_SMALL: u32 = 0xc00d07d1;
pub type Result<T> = std::result::Result<T, u32>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Reader,
    Media,
    Header,
    Writer,
    Manager,
    Buffer,
}

/// WM_MEDIA_TYPE's pointer-free fields, followed by its owned format block.
/// The guest wire header is 72 bytes; the native x64 header is larger.
#[derive(Clone, Debug, Default)]
pub struct MediaType {
    pub major: [u8; 16],
    pub subtype: [u8; 16],
    pub fixed: u32,
    pub temporal: u32,
    pub sample_size: u32,
    pub format_type: [u8; 16],
    pub format: Vec<u8>,
}
#[derive(Debug)]
pub struct Sample {
    pub bytes: Vec<u8>,
    pub time: u64,
    pub duration: u64,
    pub flags: u32,
    pub output: u32,
    pub stream: u16,
}
#[derive(Debug)]
pub enum Request {
    Create(Kind, u32),
    Release(u32),
    Open(u32, u32),
    Close(u32),
    Range(u32, u64, i64),
    Next(u32, u16),
    ReadCompressed(u32, u16, bool),
    OutputCount(u32),
    Props(u32, u32),
    FormatCount(u32, u32),
    Format(u32, u32, u32),
    SetProps(u32, u32, u32),
    StreamNumber(u32, u32),
    OutputNumber(u32, u16),
    Media(u32),
    Setting(u32, u32, Vec<u16>, u32, Vec<u8>),
    Attribute(u32, Option<u16>, Vec<u16>),
}
#[derive(Debug)]
pub enum Reply {
    Done,
    Number(u32),
    Object(u32, Kind),
    Media(MediaType),
    Sample(Sample),
    Attribute(u32, Vec<u8>),
}

/// Only these IStream operations are needed by the read-only host decoder.
/// Calls execute on the guest-lock holder, including calls requested by COM
/// codec threads. HRESULT and short reads are preserved.
pub enum StreamOp {
    Read(u32),
    Seek(i64, u32),
    Stat(u32),
}
pub struct StreamReply {
    pub hr: u32,
    pub bytes: Vec<u8>,
    pub value: u64,
}
pub trait GuestStream {
    fn call(&mut self, stream: u32, op: StreamOp) -> StreamReply;
}
pub trait Decoder: Send {
    /// Open replaces the previous source and leaves the reader closed on
    /// failure. It must finish all callbacks before the old source's guest
    /// reference is released. Close/Release likewise drain pending callbacks.
    fn request(&mut self, request: Request, guest: &mut dyn GuestStream) -> Result<Reply>;
}

struct Guest<'a> {
    m: &'a mut dyn Mem,
    c: &'a mut Cpu,
}
fn alloc(n: u32) -> Result<u32> {
    with_state(|s| s.heap.alloc(n)).ok_or(0x8007000e)
}
fn free(p: u32) {
    with_state(|s| s.heap.free(p)).expect("movie-owned heap block");
}
fn bytes(m: &dyn Mem, p: u32, n: u32) -> Vec<u8> {
    let mut v = vec![0u8; n as usize];
    m.read_bytes(p, &mut v);
    v
}
fn put(m: &mut dyn Mem, p: u32, b: &[u8]) {
    crate::fastmem::write_bulk(m, p, b);
}
fn wide(m: &dyn Mem, p: u32) -> Vec<u16> {
    let mut b = Vec::new();
    if p != 0 {
        for k in 0..32768 {
            let v = m.u16(p + 2 * k);
            if v == 0 {
                break;
            }
            b.push(v);
        }
    }
    b.push(0);
    b
}
impl GuestStream for Guest<'_> {
    fn call(&mut self, stream: u32, op: StreamOp) -> StreamReply {
        // Bound each transfer, not the movie. Codecs can request large reads;
        // the COM adapter splits those into these reusable scratch blocks.
        let size = match op {
            StreamOp::Read(n) => n.saturating_add(8),
            _ => 80,
        };
        let p = match alloc(size) {
            Ok(p) => p,
            Err(hr) => {
                return StreamReply {
                    hr,
                    bytes: vec![],
                    value: 0,
                }
            }
        };
        crate::services::zero(self.m, p, size);
        let (slot, args) = match op {
            StreamOp::Read(n) => (3, vec![stream, p, n, p + n]),
            StreamOp::Seek(d, o) => (5, vec![stream, d as u32, (d as u64 >> 32) as u32, o, p]),
            StreamOp::Stat(flags) => (12, vec![stream, p, flags]),
        };
        let target = self.m.u32(self.m.u32(stream) + slot * 4);
        let hr = call_guest(self.m, self.c, target, &args);
        let (data, value) = match op {
            StreamOp::Read(n) => {
                let got = self.m.u32(p + n);
                if got > n {
                    free(p);
                    return StreamReply {
                        hr: E_FAIL,
                        bytes: vec![],
                        value: 0,
                    };
                }
                (bytes(self.m, p, got), got as u64)
            }
            StreamOp::Seek(..) => (vec![], read64(self.m, p)),
            StreamOp::Stat(_) => (vec![], read64(self.m, p + 8)),
        };
        free(p);
        StreamReply {
            hr,
            bytes: data,
            value,
        }
    }
}

struct Group {
    host: u32,
    refs: u32,
    aliases: HashMap<Kind, u32>,
    stream: u32,
    buffer: Option<(u32, u32, u32)>,
}
#[derive(Default)]
pub(crate) struct Movies {
    backend: Option<Box<dyn Decoder>>,
    tables: HashMap<Kind, u32>,
    objects: HashMap<u32, (u32, Kind)>,
    groups: HashMap<u32, Group>,
}
fn state<R>(f: impl FnOnce(&mut Movies) -> R) -> R {
    crate::runtime::with(|r| r.movies.with(f))
}
/// Inject a decoder before creating objects (headless tests / other hosts).
pub fn set_decoder(decoder: Box<dyn Decoder>) {
    state(|s| {
        assert!(s.groups.is_empty());
        s.backend = Some(decoder)
    });
}
fn invoke(m: &mut dyn Mem, c: &mut Cpu, r: Request) -> Result<Reply> {
    // Take ownership: a guest stream callback can reenter runtime services.
    let mut decoder = state(|s| s.backend.take()).ok_or(E_FAIL)?;
    let result = decoder.request(r, &mut Guest { m, c });
    state(|s| s.backend = Some(decoder));
    result
}
fn info(p: u32) -> Result<(u32, u32, Kind)> {
    state(|s| {
        let &(id, k) = s.objects.get(&p).ok_or(E_POINTER)?;
        Ok((id, s.groups[&id].host, k))
    })
}
fn object(m: &mut dyn Mem, host: u32, kind: Kind) -> u32 {
    let v = state(|s| s.tables[&kind]);
    let p = interfaces::object(m, v, 4);
    state(|s| {
        s.objects.insert(p, (p, kind));
        s.groups.insert(
            p,
            Group {
                host,
                refs: 1,
                aliases: HashMap::from([(kind, p)]),
                stream: 0,
                buffer: None,
            },
        );
    });
    p
}
fn finish(m: &dyn Mem, c: &mut Cpu, n: u32, r: Result<()>) {
    c.eax = r.err().unwrap_or(0);
    ret(m, c, 4 * n);
}
fn out(m: &mut dyn Mem, p: u32, v: u32) -> Result<()> {
    if p == 0 {
        return Err(E_POINTER);
    }
    m.set_u32(p, v);
    Ok(())
}
fn created(m: &mut dyn Mem, c: &mut Cpu, kind: Kind, rights: u32, p: u32) -> Result<()> {
    out(m, p, 0)?;
    match invoke(m, c, Request::Create(kind, rights))? {
        Reply::Object(h, k) => {
            let o = object(m, h, k);
            out(m, p, o)
        }
        _ => Err(E_FAIL),
    }
}
pub fn create_reader(m: &mut dyn Mem, c: &mut Cpu) {
    let a = (arg(m, c, 0), arg(m, c, 1), arg(m, c, 2));
    let r = if a.0 != 0 {
        Err(E_INVALIDARG)
    } else {
        created(m, c, Kind::Reader, a.1, a.2)
    };
    finish(m, c, 3, r)
}
pub fn create_writer(m: &mut dyn Mem, c: &mut Cpu) {
    let a = (arg(m, c, 0), arg(m, c, 1));
    let r = if a.0 != 0 {
        Err(E_INVALIDARG)
    } else {
        created(m, c, Kind::Writer, 0, a.1)
    };
    finish(m, c, 2, r)
}
pub fn create_manager(m: &mut dyn Mem, c: &mut Cpu) {
    let p = arg(m, c, 0);
    let r = created(m, c, Kind::Manager, 0, p);
    finish(m, c, 1, r)
}

// Public SDK IIDs. HeaderInfo is requested at 0x00F7CBAB with the IID at
// 0x014E764C; OutputMediaProps inherits IWMMediaProps (GetMediaType +0x10).
const UNKNOWN: u128 = 0x00000000_0000_0000_c000_000000000046;
const READER: u128 = 0x9397f121_7705_4dc9_b049_98b698188414;
const HEADER: u128 = 0x96406bda_2b2b_11d3_b36b_00c04f6108ff;
const MEDIA: u128 = 0x96406bce_2b2b_11d3_b36b_00c04f6108ff;
const OUTPUT: u128 = 0x96406bd7_2b2b_11d3_b36b_00c04f6108ff;
const BUFFER: u128 = 0xe1cd3524_03d7_11d2_9eed_006097d2d7cf;
const WRITER: u128 = 0x96406bd4_2b2b_11d3_b36b_00c04f6108ff;
const MANAGER: u128 = 0xd16679f2_6ca0_472d_8d31_2f5d55aee155;
fn iid(m: &dyn Mem, p: u32) -> u128 {
    ((m.u32(p) as u128) << 96)
        | ((m.u16(p + 4) as u128) << 80)
        | ((m.u16(p + 6) as u128) << 64)
        | u64::from_be_bytes(bytes(m, p + 8, 8).try_into().unwrap()) as u128
}
fn query(m: &mut dyn Mem, c: &mut Cpu) {
    let (p, i, o) = (arg(m, c, 0), arg(m, c, 1), arg(m, c, 2));
    let r = (|| {
        out(m, o, 0)?;
        if i == 0 {
            return Err(E_POINTER);
        }
        let (id, _, k) = info(p)?;
        let guid = iid(m, i);
        let wanted = match guid {
            UNKNOWN => None,
            READER if k == Kind::Reader || k == Kind::Header => Some(Kind::Reader),
            HEADER if k == Kind::Reader || k == Kind::Header => Some(Kind::Header),
            MEDIA | OUTPUT if k == Kind::Media => Some(Kind::Media),
            BUFFER if k == Kind::Buffer => Some(k),
            WRITER if k == Kind::Writer => Some(k),
            MANAGER if k == Kind::Manager => Some(k),
            _ => return Err(E_NOINTERFACE),
        };
        let target = if let Some(w) = wanted {
            if let Some(a) = state(|s| s.groups[&id].aliases.get(&w).copied()) {
                a
            } else {
                let v = state(|s| s.tables[&w]);
                let a = interfaces::object(m, v, 4);
                state(|s| {
                    s.objects.insert(a, (id, w));
                    s.groups.get_mut(&id).unwrap().aliases.insert(w, a);
                });
                a
            }
        } else {
            id
        };
        state(|s| s.groups.get_mut(&id).unwrap().refs += 1);
        out(m, o, target)
    })();
    finish(m, c, 3, r)
}
fn addref(m: &mut dyn Mem, c: &mut Cpu) {
    let p = arg(m, c, 0);
    c.eax = info(p)
        .map(|(id, _, _)| {
            state(|s| {
                let g = s.groups.get_mut(&id).unwrap();
                g.refs += 1;
                g.refs
            })
        })
        .unwrap_or(0);
    ret(m, c, 4)
}
fn stream_ref(m: &mut dyn Mem, c: &mut Cpu, p: u32, slot: u32) {
    let f = m.u32(m.u32(p) + 4 * slot);
    call_guest(m, c, f, &[p]);
}
fn release(m: &mut dyn Mem, c: &mut Cpu) {
    let p = arg(m, c, 0);
    let mut count = 0;
    if let Ok((id, _, _)) = info(p) {
        let dead = state(|s| {
            let g = s.groups.get_mut(&id).unwrap();
            g.refs -= 1;
            count = g.refs;
            if count == 0 {
                let g = s.groups.remove(&id).unwrap();
                for p in g.aliases.values() {
                    s.objects.remove(p);
                }
                Some(g)
            } else {
                None
            }
        });
        if let Some(g) = dead {
            if g.host != 0 {
                let _ = invoke(m, c, Request::Release(g.host));
            }
            if g.stream != 0 {
                stream_ref(m, c, g.stream, 2);
            }
            if let Some((p, _, _)) = g.buffer {
                free(p);
            }
            for p in g.aliases.values() {
                free(*p);
            }
        }
    }
    c.eax = count;
    ret(m, c, 4)
}

fn reader_open(m: &mut dyn Mem, c: &mut Cpu) {
    let (p, s) = (arg(m, c, 0), arg(m, c, 1));
    let r = (|| {
        if s == 0 {
            return Err(E_POINTER);
        }
        let (id, h, _) = info(p)?;
        let old = state(|st| st.groups[&id].stream);
        stream_ref(m, c, s, 1);
        let r = invoke(m, c, Request::Open(h, s));
        // Native OpenStream can replace a source. Keep both sources alive
        // through its callbacks; the backend closes on a failed open.
        state(|st| st.groups.get_mut(&id).unwrap().stream = if r.is_ok() { s } else { 0 });
        if old != 0 {
            stream_ref(m, c, old, 2);
        }
        if r.is_err() {
            stream_ref(m, c, s, 2);
        }
        r.map(|_| ())
    })();
    finish(m, c, 2, r)
}
fn reader_close(m: &mut dyn Mem, c: &mut Cpu) {
    let p = arg(m, c, 0);
    let r = (|| {
        let (id, h, _) = info(p)?;
        invoke(m, c, Request::Close(h))?;
        let stream = state(|s| std::mem::take(&mut s.groups.get_mut(&id).unwrap().stream));
        if stream != 0 {
            stream_ref(m, c, stream, 2);
        }
        Ok(())
    })();
    finish(m, c, 1, r)
}
fn range(m: &mut dyn Mem, c: &mut Cpu) {
    let (p, t, d) = (arg(m, c, 0), read64(m, c.esp + 8), read64(m, c.esp + 16));
    let r = info(p).and_then(|(_, h, _)| invoke(m, c, Request::Range(h, t, d as i64)).map(|_| ()));
    finish(m, c, 5, r)
}
fn compressed(m: &mut dyn Mem, c: &mut Cpu) {
    let (p, s, b) = (arg(m, c, 0), arg(m, c, 1), arg(m, c, 2));
    let r =
        info(p).and_then(|(_, h, _)| invoke(m, c, Request::ReadCompressed(h, s as u16, b != 0)).map(|_| ()));
    finish(m, c, 3, r)
}
fn number(m: &mut dyn Mem, c: &mut Cpu, which: u32) {
    let p = arg(m, c, 0);
    let a = arg(m, c, 1);
    let o = arg(m, c, if which == 0 { 1 } else { 2 });
    let r = (|| {
        if o == 0 {
            return Err(E_POINTER);
        }
        let (_, h, _) = info(p)?;
        let q = match which {
            0 => Request::OutputCount(h),
            1 => Request::FormatCount(h, a),
            2 => Request::StreamNumber(h, a),
            _ => Request::OutputNumber(h, a as u16),
        };
        match invoke(m, c, q)? {
            Reply::Number(v) => {
                if which == 2 {
                    m.set_u16(o, v as u16)
                } else {
                    m.set_u32(o, v)
                }
                Ok(())
            }
            _ => Err(E_FAIL),
        }
    })();
    finish(m, c, if which == 0 { 2 } else { 3 }, r)
}
fn output_count(m: &mut dyn Mem, c: &mut Cpu) {
    number(m, c, 0)
}
fn format_count(m: &mut dyn Mem, c: &mut Cpu) {
    number(m, c, 1)
}
fn stream_number(m: &mut dyn Mem, c: &mut Cpu) {
    number(m, c, 2)
}
fn output_number(m: &mut dyn Mem, c: &mut Cpu) {
    number(m, c, 3)
}
fn props(m: &mut dyn Mem, c: &mut Cpu, format: bool) {
    let (p, a, b, o) = (
        arg(m, c, 0),
        arg(m, c, 1),
        arg(m, c, 2),
        arg(m, c, if format { 3 } else { 2 }),
    );
    let r = (|| {
        out(m, o, 0)?;
        let (_, h, _) = info(p)?;
        match invoke(
            m,
            c,
            if format {
                Request::Format(h, a, b)
            } else {
                Request::Props(h, a)
            },
        )? {
            Reply::Object(h, k) => {
                let p = object(m, h, k);
                out(m, o, p)
            }
            _ => Err(E_FAIL),
        }
    })();
    finish(m, c, if format { 4 } else { 3 }, r)
}
fn get_props(m: &mut dyn Mem, c: &mut Cpu) {
    props(m, c, false)
}
fn get_format(m: &mut dyn Mem, c: &mut Cpu) {
    props(m, c, true)
}
fn set_props(m: &mut dyn Mem, c: &mut Cpu) {
    let (p, a, o) = (arg(m, c, 0), arg(m, c, 1), arg(m, c, 2));
    let r = (|| {
        let (_, h, _) = info(p)?;
        let (_, v, k) = info(o)?;
        if k != Kind::Media {
            return Err(E_INVALIDARG);
        }
        invoke(m, c, Request::SetProps(h, a, v)).map(|_| ())
    })();
    finish(m, c, 3, r)
}
fn setting(m: &mut dyn Mem, c: &mut Cpu) {
    let a: Vec<_> = (0..6).map(|i| arg(m, c, i)).collect();
    let r = (|| {
        if a[2] == 0 || (a[5] != 0 && a[4] == 0) {
            return Err(E_POINTER);
        }
        let (_, h, _) = info(a[0])?;
        let q = Request::Setting(h, a[1], wide(m, a[2]), a[3], bytes(m, a[4], a[5] as u16 as u32));
        invoke(m, c, q).map(|_| ())
    })();
    finish(m, c, 6, r)
}
fn get_media(m: &mut dyn Mem, c: &mut Cpu) {
    let (p, b, n) = (arg(m, c, 0), arg(m, c, 1), arg(m, c, 2));
    let r = (|| {
        if n == 0 {
            return Err(E_INVALIDARG);
        }
        let (_, h, _) = info(p)?;
        let Reply::Media(v) = invoke(m, c, Request::Media(h))? else {
            return Err(E_FAIL);
        };
        let needed = 72 + v.format.len() as u32;
        let cap = if b == 0 { 0 } else { m.u32(n) };
        m.set_u32(n, needed);
        if b == 0 {
            return Ok(());
        }
        if cap < needed {
            return Err(BUFFER_TOO_SMALL);
        }
        put(m, b, &v.major);
        put(m, b + 16, &v.subtype);
        m.set_u32(b + 32, v.fixed);
        m.set_u32(b + 36, v.temporal);
        m.set_u32(b + 40, v.sample_size);
        put(m, b + 44, &v.format_type);
        m.set_u32(b + 60, 0);
        m.set_u32(b + 64, v.format.len() as u32);
        m.set_u32(b + 68, if v.format.is_empty() { 0 } else { b + 72 });
        put(m, b + 72, &v.format);
        Ok(())
    })();
    finish(m, c, 3, r)
}
fn media_type(m: &mut dyn Mem, c: &mut Cpu) {
    let (p, o) = (arg(m, c, 0), arg(m, c, 1));
    let r = (|| {
        if o == 0 {
            return Err(E_POINTER);
        }
        let (_, h, _) = info(p)?;
        let Reply::Media(v) = invoke(m, c, Request::Media(h))? else {
            return Err(E_FAIL);
        };
        put(m, o, &v.major);
        Ok(())
    })();
    finish(m, c, 2, r)
}
fn attribute(m: &mut dyn Mem, c: &mut Cpu) {
    let a: Vec<_> = (0..6).map(|i| arg(m, c, i)).collect();
    let r = (|| {
        if a[2] == 0 || a[3] == 0 || a[5] == 0 {
            return Err(E_POINTER);
        }
        let (_, h, _) = info(a[0])?;
        let req = Request::Attribute(h, if a[1] == 0 { None } else { Some(m.u16(a[1])) }, wide(m, a[2]));
        let Reply::Attribute(t, b) = invoke(m, c, req)? else {
            return Err(E_FAIL);
        };
        let cap = m.u16(a[5]) as usize;
        m.set_u32(a[3], t);
        m.set_u16(a[5], b.len() as u16);
        if a[4] == 0 {
            return Ok(());
        }
        if cap < b.len() {
            return Err(BUFFER_TOO_SMALL);
        }
        put(m, a[4], &b);
        Ok(())
    })();
    finish(m, c, 6, r)
}
fn next(m: &mut dyn Mem, c: &mut Cpu) {
    let a: Vec<_> = (0..8).map(|i| arg(m, c, i)).collect();
    let r = (|| {
        out(m, a[2], 0)?;
        let (_, h, _) = info(a[0])?;
        let Reply::Sample(v) = invoke(m, c, Request::Next(h, a[1] as u16))? else {
            return Err(E_FAIL);
        };
        let b = alloc(v.bytes.len() as u32)?;
        put(m, b, &v.bytes);
        let p = object(m, 0, Kind::Buffer);
        state(|s| {
            s.groups.get_mut(&p).unwrap().buffer = Some((b, v.bytes.len() as u32, v.bytes.len() as u32))
        });
        m.set_u32(a[2], p);
        if a[3] != 0 {
            write64(m, a[3], v.time)
        }
        if a[4] != 0 {
            write64(m, a[4], v.duration)
        }
        if a[5] != 0 {
            m.set_u32(a[5], v.flags)
        }
        if a[6] != 0 {
            m.set_u32(a[6], v.output)
        }
        if a[7] != 0 {
            m.set_u16(a[7], v.stream)
        }
        Ok(())
    })();
    finish(m, c, 8, r)
}
fn buffer(m: &mut dyn Mem, c: &mut Cpu, slot: u32) {
    let (p, o) = (arg(m, c, 0), arg(m, c, 1));
    let n = if slot == 7 { arg(m, c, 2) } else { 0 };
    let r = (|| {
        let (id, _, k) = info(p)?;
        if k != Kind::Buffer {
            return Err(E_INVALIDARG);
        }
        let (b, len, max) = state(|s| s.groups[&id].buffer.unwrap());
        match slot {
            3 => out(m, o, len),
            4 => {
                if o > max {
                    return Err(E_INVALIDARG);
                }
                state(|s| s.groups.get_mut(&id).unwrap().buffer.as_mut().unwrap().1 = o);
                Ok(())
            }
            5 => out(m, o, max),
            6 => out(m, o, b),
            7 => {
                if o == 0 || n == 0 {
                    return Err(E_POINTER);
                }
                out(m, o, b)?;
                out(m, n, len)
            }
            _ => unreachable!(),
        }
    })();
    finish(m, c, if slot == 7 { 3 } else { 2 }, r)
}
fn length(m: &mut dyn Mem, c: &mut Cpu) {
    buffer(m, c, 3)
}
fn set_length(m: &mut dyn Mem, c: &mut Cpu) {
    buffer(m, c, 4)
}
fn max_length(m: &mut dyn Mem, c: &mut Cpu) {
    buffer(m, c, 5)
}
fn get_buffer(m: &mut dyn Mem, c: &mut Cpu) {
    buffer(m, c, 6)
}
fn buffer_length(m: &mut dyn Mem, c: &mut Cpu) {
    buffer(m, c, 7)
}

pub fn install(m: &mut dyn Mem, r: &mut Registry, t: &mut Tokens) -> std::result::Result<(), Conflict> {
    let specs = [
        (
            Kind::Reader,
            Spec {
                name: "IWMSyncReader",
                slots: 24,
                names: &[
                    "QueryInterface",
                    "AddRef",
                    "Release",
                    "Open",
                    "Close",
                    "SetRange",
                    "SetRangeByFrame",
                    "GetNextSample",
                    "SetStreamsSelected",
                    "GetStreamSelected",
                    "SetReadStreamSamples",
                    "GetReadStreamSamples",
                    "GetOutputSetting",
                    "SetOutputSetting",
                    "GetOutputCount",
                    "GetOutputProps",
                    "SetOutputProps",
                    "GetOutputFormatCount",
                    "GetOutputFormat",
                    "GetOutputNumberForStream",
                    "GetStreamNumberForOutput",
                    "GetMaxOutputSampleSize",
                    "GetMaxStreamSampleSize",
                    "OpenStream",
                ],
                methods: &[
                    (0, query),
                    (1, addref),
                    (2, release),
                    (4, reader_close),
                    (5, range),
                    (7, next),
                    (10, compressed),
                    (13, setting),
                    (14, output_count),
                    (15, get_props),
                    (16, set_props),
                    (17, format_count),
                    (18, get_format),
                    (19, output_number),
                    (20, stream_number),
                    (23, reader_open),
                ],
            },
        ),
        (
            Kind::Media,
            Spec {
                name: "IWMOutputMediaProps",
                slots: 8,
                names: &[
                    "QueryInterface",
                    "AddRef",
                    "Release",
                    "GetType",
                    "GetMediaType",
                    "SetMediaType",
                    "GetStreamGroupName",
                    "GetConnectionName",
                ],
                methods: &[
                    (0, query),
                    (1, addref),
                    (2, release),
                    (3, media_type),
                    (4, get_media),
                ],
            },
        ),
        (
            Kind::Header,
            Spec {
                name: "IWMHeaderInfo",
                slots: 15,
                names: &[
                    "QueryInterface",
                    "AddRef",
                    "Release",
                    "GetAttributeCount",
                    "GetAttributeByIndex",
                    "GetAttributeByName",
                ],
                methods: &[(0, query), (1, addref), (2, release), (5, attribute)],
            },
        ),
        (
            Kind::Buffer,
            Spec {
                name: "INSSBuffer",
                slots: 8,
                names: &[
                    "QueryInterface",
                    "AddRef",
                    "Release",
                    "GetLength",
                    "SetLength",
                    "GetMaxLength",
                    "GetBuffer",
                    "GetBufferAndLength",
                ],
                methods: &[
                    (0, query),
                    (1, addref),
                    (2, release),
                    (3, length),
                    (4, set_length),
                    (5, max_length),
                    (6, get_buffer),
                    (7, buffer_length),
                ],
            },
        ),
        (
            Kind::Writer,
            Spec {
                name: "IWMWriter",
                slots: 16,
                names: &[
                    "QueryInterface",
                    "AddRef",
                    "Release",
                    "SetProfileByID",
                    "SetProfile",
                    "SetOutputFilename",
                    "GetInputCount",
                    "GetInputProps",
                    "SetInputProps",
                    "GetInputFormatCount",
                    "GetInputFormat",
                    "BeginWriting",
                    "EndWriting",
                    "AllocateSample",
                    "WriteSample",
                    "Flush",
                ],
                methods: &[(0, query), (1, addref), (2, release)],
            },
        ),
        (
            Kind::Manager,
            Spec {
                name: "IWMProfileManager",
                slots: 9,
                names: &[
                    "QueryInterface",
                    "AddRef",
                    "Release",
                    "CreateEmptyProfile",
                    "LoadProfileByID",
                    "LoadProfileByData",
                    "SaveProfile",
                    "GetSystemProfileCount",
                    "LoadSystemProfile",
                ],
                methods: &[(0, query), (1, addref), (2, release)],
            },
        ),
    ];
    for (k, s) in specs {
        let v = interfaces::vtable(m, r, t, &s)?;
        state(|s| {
            s.tables.insert(k, v);
        });
    }
    let v = interfaces::vtable(
        m,
        r,
        t,
        &Spec {
            name: "WMVCore",
            slots: 3,
            names: &["WMCreateSyncReader", "WMCreateWriter", "WMCreateProfileManager"],
            methods: &[(0, create_reader), (1, create_writer), (2, create_manager)],
        },
    )?;
    for (k, n) in ["WMCreateSyncReader", "WMCreateWriter", "WMCreateProfileManager"]
        .iter()
        .enumerate()
    {
        let token = m.u32(v + k as u32 * 4);
        with_state(|s| {
            s.exports.insert(("wmvcore.dll".into(), n.to_string()), token);
        });
    }
    #[cfg(windows)]
    state(|s| s.backend = Some(Box::new(windows::SystemDecoder::new())));
    Ok(())
}

fn read64(m: &dyn Mem, p: u32) -> u64 {
    m.u32(p) as u64 | ((m.u32(p + 4) as u64) << 32)
}
fn write64(m: &mut dyn Mem, p: u32, v: u64) {
    m.set_u32(p, v as u32);
    m.set_u32(p + 4, (v >> 32) as u32);
}
