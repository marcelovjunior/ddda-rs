//! System WMVCore on one MTA thread. COM pointers never leave that thread.
//! Its IStream sends read/seek/stat requests to the guest-lock holder;
//! neither COM nor its asynchronous I/O threads dereference guest memory.
use super::*;
use ::windows::{
    core::{implement, IUnknown, Interface, Ref, GUID, HRESULT, PCWSTR},
    Win32::{Media::WindowsMediaFormat::*, System::Com::*},
};
use std::{
    ffi::c_void,
    sync::mpsc::{self, Receiver, Sender, SyncSender},
};

enum Event {
    Stream(u32, StreamOp, SyncSender<StreamReply>),
    Done(Result<Reply>),
}
pub(super) struct SystemDecoder {
    commands: Option<Sender<Request>>,
    events: Receiver<Event>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl SystemDecoder {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let (events, out) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("ddda-wmvcore".into())
            .spawn(move || {
                let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let mut host = Host {
                    objects: HashMap::new(),
                    next: 1,
                    events: events.clone(),
                };
                while let Ok(request) = rx.recv() {
                    let reply = if initialized.is_err() {
                        Err(initialized.0 as u32)
                    } else {
                        unsafe { host.request(request) }.map_err(|e| e.code().0 as u32)
                    };
                    if events.send(Event::Done(reply)).is_err() {
                        break;
                    }
                }
                drop(host);
                if initialized.is_ok() {
                    unsafe { CoUninitialize() }
                }
            })
            .expect("WMVCore worker");
        Self {
            commands: Some(tx),
            events: out,
            worker: Some(worker),
        }
    }
}
impl Decoder for SystemDecoder {
    fn request(&mut self, r: Request, g: &mut dyn GuestStream) -> Result<Reply> {
        self.commands
            .as_ref()
            .ok_or(E_FAIL)?
            .send(r)
            .map_err(|_| E_FAIL)?;
        loop {
            match self.events.recv().map_err(|_| E_FAIL)? {
                Event::Done(r) => return r,
                Event::Stream(s, op, reply) => {
                    let _ = reply.send(g.call(s, op));
                }
            }
        }
    }
}
impl Drop for SystemDecoder {
    fn drop(&mut self) {
        self.commands.take();
        // A codec can have queued an asynchronous read between guest calls.
        // Reject it during shutdown so releasing the reader cannot deadlock.
        if let Some(worker) = self.worker.take() {
            while !worker.is_finished() {
                match self.events.recv_timeout(std::time::Duration::from_millis(10)) {
                    Ok(Event::Stream(_, _, reply)) => {
                        let _ = reply.send(StreamReply {
                            hr: E_FAIL,
                            bytes: vec![],
                            value: 0,
                        });
                    }
                    _ => {}
                }
            }
            let _ = worker.join();
        }
    }
}
#[implement(IStream)]
struct Stream {
    guest: u32,
    events: Sender<Event>,
}
impl Stream {
    fn request(&self, op: StreamOp) -> StreamReply {
        let (tx, rx) = mpsc::sync_channel(1);
        if self.events.send(Event::Stream(self.guest, op, tx)).is_ok() {
            if let Ok(r) = rx.recv() {
                return r;
            }
        }
        StreamReply {
            hr: E_FAIL,
            bytes: vec![],
            value: 0,
        }
    }
}
impl ISequentialStream_Impl for Stream_Impl {
    fn Read(&self, p: *mut c_void, n: u32, got: *mut u32) -> HRESULT {
        if !got.is_null() {
            unsafe { *got = 0 }
        }
        if n != 0 && p.is_null() {
            return HRESULT(E_POINTER as i32);
        }
        let mut done = 0;
        while done < n {
            let count = (n - done).min(64 * 1024);
            let r = self.request(StreamOp::Read(count));
            if r.bytes.len() > count as usize {
                return HRESULT(E_FAIL as i32);
            }
            if !r.bytes.is_empty() {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        r.bytes.as_ptr(),
                        p.cast::<u8>().add(done as usize),
                        r.bytes.len(),
                    );
                }
            }
            done += r.bytes.len() as u32;
            if !got.is_null() {
                unsafe { *got = done }
            }
            if (r.hr as i32) < 0 {
                return HRESULT(r.hr as i32);
            }
            if r.bytes.len() < count as usize || r.hr != 0 {
                // The game's memory-backed IStream returns S_OK for a
                // partial final read (0x00F7B792), S_FALSE only at EOF.
                return HRESULT(r.hr as i32);
            }
        }
        HRESULT(0)
    }
    fn Write(&self, _: *const c_void, _: u32, got: *mut u32) -> HRESULT {
        if !got.is_null() {
            unsafe { *got = 0 }
        }
        HRESULT(0x80030005u32 as i32)
    }
}
fn unsupported<T>() -> ::windows::core::Result<T> {
    Err(HRESULT(E_NOTIMPL as i32).into())
}
impl IStream_Impl for Stream_Impl {
    fn Seek(&self, d: i64, o: STREAM_SEEK, p: *mut u64) -> ::windows::core::Result<()> {
        let r = self.request(StreamOp::Seek(d, o.0));
        HRESULT(r.hr as i32).ok()?;
        if !p.is_null() {
            unsafe { *p = r.value }
        }
        Ok(())
    }
    fn Stat(&self, p: *mut STATSTG, flags: &STATFLAG) -> ::windows::core::Result<()> {
        if p.is_null() {
            return Err(HRESULT(E_POINTER as i32).into());
        }
        let r = self.request(StreamOp::Stat(flags.0 as u32));
        HRESULT(r.hr as i32).ok()?;
        unsafe {
            *p = STATSTG {
                r#type: STGTY_STREAM.0 as u32,
                cbSize: r.value,
                ..Default::default()
            };
        }
        Ok(())
    }
    fn SetSize(&self, _: u64) -> ::windows::core::Result<()> {
        unsupported()
    }
    fn CopyTo(&self, _: Ref<IStream>, _: u64, _: *mut u64, _: *mut u64) -> ::windows::core::Result<()> {
        unsupported()
    }
    fn Commit(&self, _: &STGC) -> ::windows::core::Result<()> {
        unsupported()
    }
    fn Revert(&self) -> ::windows::core::Result<()> {
        unsupported()
    }
    fn LockRegion(&self, _: u64, _: u64, _: &LOCKTYPE) -> ::windows::core::Result<()> {
        unsupported()
    }
    fn UnlockRegion(&self, _: u64, _: u64, _: u32) -> ::windows::core::Result<()> {
        unsupported()
    }
    fn Clone(&self) -> ::windows::core::Result<IStream> {
        unsupported()
    }
}

struct Host {
    objects: HashMap<u32, IUnknown>,
    next: u32,
    events: Sender<Event>,
}
impl Host {
    fn store<T: Interface>(&mut self, v: T, k: Kind) -> ::windows::core::Result<Reply> {
        let id = self.next;
        self.next += 1;
        self.objects.insert(id, v.cast()?);
        Ok(Reply::Object(id, k))
    }
    fn get<T: Interface>(&self, id: u32) -> ::windows::core::Result<T> {
        self.objects
            .get(&id)
            .ok_or_else(|| ::windows::core::Error::from_hresult(HRESULT(E_POINTER as i32)))?
            .cast()
    }
    unsafe fn request(&mut self, q: Request) -> ::windows::core::Result<Reply> {
        unsafe {
            match q {
                Request::Create(Kind::Reader, rights) => {
                    let v = WMCreateSyncReader(None, rights)?;
                    self.store(v, Kind::Reader)
                }
                Request::Create(Kind::Writer, _) => {
                    let v = WMCreateWriter(None)?;
                    self.store(v, Kind::Writer)
                }
                Request::Create(Kind::Manager, _) => {
                    let v = WMCreateProfileManager()?;
                    self.store(v, Kind::Manager)
                }
                Request::Create(..) => unsupported(),
                Request::Release(h) => {
                    if let Some(object) = self.objects.remove(&h) {
                        if let Ok(reader) = object.cast::<IWMSyncReader>() {
                            let _ = reader.Close();
                        }
                    }
                    Ok(Reply::Done)
                }
                Request::Open(h, s) => {
                    let stream: IStream = Stream {
                        guest: s,
                        events: self.events.clone(),
                    }
                    .into();
                    let reader = self.get::<IWMSyncReader>(h)?;
                    if let Err(error) = reader.OpenStream(&stream) {
                        let _ = reader.Close();
                        return Err(error);
                    }
                    Ok(Reply::Done)
                }
                Request::Close(h) => {
                    self.get::<IWMSyncReader>(h)?.Close()?;
                    Ok(Reply::Done)
                }
                Request::Range(h, t, d) => {
                    self.get::<IWMSyncReader>(h)?.SetRange(t, d)?;
                    Ok(Reply::Done)
                }
                Request::ReadCompressed(h, s, b) => {
                    self.get::<IWMSyncReader>(h)?.SetReadStreamSamples(s, b)?;
                    Ok(Reply::Done)
                }
                Request::OutputCount(h) => Ok(Reply::Number(self.get::<IWMSyncReader>(h)?.GetOutputCount()?)),
                Request::FormatCount(h, o) => Ok(Reply::Number(
                    self.get::<IWMSyncReader>(h)?.GetOutputFormatCount(o)?,
                )),
                Request::StreamNumber(h, o) => Ok(Reply::Number(
                    self.get::<IWMSyncReader>(h)?.GetStreamNumberForOutput(o)? as u32,
                )),
                Request::OutputNumber(h, s) => Ok(Reply::Number(
                    self.get::<IWMSyncReader>(h)?.GetOutputNumberForStream(s)?,
                )),
                Request::Props(h, o) => {
                    let p = self.get::<IWMSyncReader>(h)?.GetOutputProps(o)?;
                    self.store(p, Kind::Media)
                }
                Request::Format(h, o, f) => {
                    let p = self.get::<IWMSyncReader>(h)?.GetOutputFormat(o, f)?;
                    self.store(p, Kind::Media)
                }
                Request::SetProps(h, o, p) => {
                    self.get::<IWMSyncReader>(h)?
                        .SetOutputProps(o, &self.get::<IWMOutputMediaProps>(p)?)?;
                    Ok(Reply::Done)
                }
                Request::Setting(h, o, n, t, b) => {
                    self.get::<IWMSyncReader>(h)?.SetOutputSetting(
                        o,
                        PCWSTR(n.as_ptr()),
                        WMT_ATTR_DATATYPE(t as i32),
                        &b,
                    )?;
                    Ok(Reply::Done)
                }
                Request::Media(h) => {
                    let p = self.get::<IWMMediaProps>(h)?;
                    let mut n = 0;
                    p.GetMediaType(std::ptr::null_mut(), &mut n)?;
                    let mut buf = vec![0u64; (n as usize + 7) / 8];
                    let mt = buf.as_mut_ptr().cast::<WM_MEDIA_TYPE>();
                    p.GetMediaType(mt, &mut n)?;
                    let v = &*mt;
                    let format = if v.cbFormat == 0 {
                        vec![]
                    } else {
                        std::slice::from_raw_parts(v.pbFormat, v.cbFormat as usize).to_vec()
                    };
                    let result = MediaType {
                        major: guid_bytes(v.majortype),
                        subtype: guid_bytes(v.subtype),
                        fixed: v.bFixedSizeSamples.0 as u32,
                        temporal: v.bTemporalCompression.0 as u32,
                        sample_size: v.lSampleSize,
                        format_type: guid_bytes(v.formattype),
                        format,
                    };
                    // GetMediaType gives the caller a reference to pUnk, if any.
                    std::mem::ManuallyDrop::drop(&mut (*mt).pUnk);
                    Ok(Reply::Media(result))
                }
                Request::Attribute(h, mut stream, n) => {
                    let p = self.get::<IWMHeaderInfo>(h)?;
                    let s = stream.as_mut().map_or(std::ptr::null_mut(), |s| s as *mut u16);
                    let (mut t, mut len) = (WMT_ATTR_DATATYPE(0), 0u16);
                    p.GetAttributeByName(s, PCWSTR(n.as_ptr()), &mut t, std::ptr::null_mut(), &mut len)?;
                    let mut b = vec![0; len as usize];
                    p.GetAttributeByName(s, PCWSTR(n.as_ptr()), &mut t, b.as_mut_ptr(), &mut len)?;
                    b.truncate(len as usize);
                    Ok(Reply::Attribute(t.0 as u32, b))
                }
                Request::Next(h, s) => {
                    let (mut sample, mut time, mut duration, mut flags, mut output, mut stream) =
                        (None, 0, 0, 0, 0, 0);
                    self.get::<IWMSyncReader>(h)?.GetNextSample(
                        s,
                        &mut sample,
                        &mut time,
                        &mut duration,
                        &mut flags,
                        &mut output,
                        &mut stream,
                    )?;
                    let sample =
                        sample.ok_or_else(|| ::windows::core::Error::from_hresult(HRESULT(E_FAIL as i32)))?;
                    let (mut data, mut len) = (std::ptr::null_mut(), 0);
                    sample.GetBufferAndLength(&mut data, &mut len)?;
                    let bytes = if len == 0 {
                        vec![]
                    } else {
                        std::slice::from_raw_parts(data, len as usize).to_vec()
                    };
                    Ok(Reply::Sample(Sample {
                        bytes,
                        time,
                        duration,
                        flags,
                        output,
                        stream,
                    }))
                }
            }
        }
    }
}
fn guid_bytes(g: GUID) -> [u8; 16] {
    let mut b = [0; 16];
    b[..4].copy_from_slice(&g.data1.to_le_bytes());
    b[4..6].copy_from_slice(&g.data2.to_le_bytes());
    b[6..8].copy_from_slice(&g.data3.to_le_bytes());
    b[8..].copy_from_slice(&g.data4);
    b
}
