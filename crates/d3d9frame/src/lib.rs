//! Captured Direct3D 9 backend command streams (brief L41,
//! docs/FRAME_ORACLE.md): every command the runtime gives its graphics
//! backend (`d3d9cmd`), written to a compact file as it happens, read back
//! as a stream of [`Command`]s and replayed into any backend.
//!
//! - [`Capture`] is a `GraphicsBackend` that records each command and
//!   passes it on to the backend it wraps (ddruntime installs it when
//!   `DDDA_CAPTURE` names a file).
//! - [`Reader`] streams a file's commands.
//! - [`oracle::compare`] replays a stream into two backends (the wgpu
//!   backend and the host's D3D9 driver) and compares their targets frame
//!   by frame, naming the first command whose target differs.
//!
//! File: `DDFRAME\0`, a version word, then records (a tag byte, a length
//! word, the payload). Large payloads (image and buffer bytes, shader
//! tokens, constant blocks, render-state lists, vertex declarations, inline
//! vertices and indices) are blobs: written once, deflated, and named by
//! number afterwards, so what repeats across draws and frames costs a word.
//! The file is flushed at every Present, so a session that stops keeps its
//! complete frames.
use d3d9cmd::{
    ClearCall, Constants, CopyCall, DrawCall, FillCall, GammaRamp, GraphicsBackend, Indices, Target, TargetTexture, TexKind, TextureBinding, Upload,
    Vertices,
};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

pub mod image;
pub mod oracle;

const MAGIC: &[u8; 8] = b"DDFRAME\0";
const VERSION: u32 = 1;
const TAG_BLOB: u8 = 1;
const TAG_COMMAND: u8 = 2;
/// "no blob" (an absent shader, gamma ramp)
const NONE: u32 = u32::MAX;

/// One backend command, as captured.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Upload(Upload),
    Draw(DrawCall),
    Clear(ClearCall),
    Present(Target),
    Copy(CopyCall),
    Fill(FillCall),
    /// GetRenderTargetData (changes nothing; kept for completeness)
    ReadBack(Target),
    QueryBegin(u32),
    QueryEnd(u32),
    /// GetData on an occlusion query
    QueryResult(u32),
    Gamma(Option<Box<GammaRamp>>),
    Release(u32),
}

impl Command {
    /// Give the command to `backend`.
    pub fn apply(&self, backend: &mut dyn GraphicsBackend) {
        match self {
            Command::Upload(u) => backend.upload(u.clone()),
            Command::Draw(d) => backend.draw(d),
            Command::Clear(c) => backend.clear(c),
            Command::Present(t) => backend.present(t),
            Command::Copy(c) => backend.copy(c),
            Command::Fill(f) => backend.fill(f),
            Command::ReadBack(t) => {
                backend.read_back(t);
            }
            Command::QueryBegin(q) => backend.query_begin(*q),
            Command::QueryEnd(q) => backend.query_end(*q),
            Command::QueryResult(q) => {
                backend.query_result(*q);
            }
            Command::Gamma(r) => backend.gamma(r.as_deref()),
            Command::Release(r) => backend.release(*r),
        }
    }

    /// The command's name in reports.
    pub fn kind(&self) -> &'static str {
        match self {
            Command::Upload(_) => "upload",
            Command::Draw(_) => "draw",
            Command::Clear(_) => "clear",
            Command::Present(_) => "present",
            Command::Copy(_) => "copy",
            Command::Fill(_) => "fill",
            Command::ReadBack(_) => "read back",
            Command::QueryBegin(_) => "query begin",
            Command::QueryEnd(_) => "query end",
            Command::QueryResult(_) => "query result",
            Command::Gamma(_) => "gamma",
            Command::Release(_) => "release",
        }
    }

    /// The colour and depth targets the command writes.
    pub fn written(&self) -> Vec<&Target> {
        match self {
            Command::Draw(d) => d.targets.iter().flatten().chain(d.depth.as_ref()).collect(),
            Command::Clear(c) => {
                // D3DCLEAR_TARGET 1, D3DCLEAR_ZBUFFER 2, D3DCLEAR_STENCIL 4
                let mut v: Vec<&Target> = if c.flags & 1 != 0 { c.targets.iter().flatten().collect() } else { vec![] };
                if c.flags & 6 != 0 {
                    v.extend(c.depth.as_ref());
                }
                v
            }
            Command::Copy(c) => vec![&c.dst],
            Command::Fill(f) => vec![&f.dst],
            _ => vec![],
        }
    }
}

// --- writing -------------------------------------------------------------------

/// A borrowed command, for writing without copying.
#[derive(Clone, Copy, Debug)]
pub enum Cmd<'a> {
    Upload(&'a Upload),
    Draw(&'a DrawCall),
    Clear(&'a ClearCall),
    Present(&'a Target),
    Copy(&'a CopyCall),
    Fill(&'a FillCall),
    ReadBack(&'a Target),
    QueryBegin(u32),
    QueryEnd(u32),
    QueryResult(u32),
    Gamma(Option<&'a GammaRamp>),
    Release(u32),
}

impl Command {
    pub fn as_cmd(&self) -> Cmd<'_> {
        match self {
            Command::Upload(u) => Cmd::Upload(u),
            Command::Draw(d) => Cmd::Draw(d),
            Command::Clear(c) => Cmd::Clear(c),
            Command::Present(t) => Cmd::Present(t),
            Command::Copy(c) => Cmd::Copy(c),
            Command::Fill(f) => Cmd::Fill(f),
            Command::ReadBack(t) => Cmd::ReadBack(t),
            Command::QueryBegin(q) => Cmd::QueryBegin(*q),
            Command::QueryEnd(q) => Cmd::QueryEnd(*q),
            Command::QueryResult(q) => Cmd::QueryResult(*q),
            Command::Gamma(r) => Cmd::Gamma(r.as_deref()),
            Command::Release(r) => Cmd::Release(*r),
        }
    }
}

/// Writes a capture file.
pub struct Writer<W: Write = BufWriter<std::fs::File>> {
    out: W,
    /// blob number by its content's 128-bit hash
    blobs: HashMap<(u64, u64), u32>,
    next_blob: u32,
    /// bytes of blobs before deflating, and as written (statistics)
    pub raw_bytes: u64,
    pub written_bytes: u64,
    pub commands: u64,
}

impl Writer {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Writer> {
        Writer::new(BufWriter::with_capacity(1 << 20, std::fs::File::create(path)?))
    }
}

/// Two SipHash-1-3 hashes with different keys (std's DefaultHasher seeded
/// by a prefix): a 128-bit content name.
fn content_hash(bytes: &[u8]) -> (u64, u64) {
    let h = |seed: u64| {
        let mut s = std::collections::hash_map::DefaultHasher::new();
        seed.hash(&mut s);
        bytes.hash(&mut s);
        s.finish()
    };
    (h(0x9E37_79B9_7F4A_7C15), h(0xC2B2_AE3D_27D4_EB4F))
}

impl<W: Write> Writer<W> {
    pub fn new(mut out: W) -> io::Result<Writer<W>> {
        out.write_all(MAGIC)?;
        out.write_all(&VERSION.to_le_bytes())?;
        Ok(Writer { out, blobs: HashMap::new(), next_blob: 0, raw_bytes: 0, written_bytes: 12, commands: 0 })
    }

    fn record(&mut self, tag: u8, payload: &[u8]) -> io::Result<()> {
        self.out.write_all(&[tag])?;
        self.out.write_all(&(payload.len() as u32).to_le_bytes())?;
        self.out.write_all(payload)?;
        self.written_bytes += 5 + payload.len() as u64;
        Ok(())
    }

    /// A blob's number, writing it the first time its content appears.
    fn blob(&mut self, bytes: &[u8]) -> io::Result<u32> {
        let key = content_hash(bytes);
        if let Some(&id) = self.blobs.get(&key) {
            return Ok(id);
        }
        let id = self.next_blob;
        self.next_blob += 1;
        let deflated = miniz_oxide::deflate::compress_to_vec(bytes, 1);
        let mut payload = Vec::with_capacity(8 + deflated.len());
        payload.extend_from_slice(&id.to_le_bytes());
        payload.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        payload.extend_from_slice(&deflated);
        self.record(TAG_BLOB, &payload)?;
        self.raw_bytes += bytes.len() as u64;
        self.blobs.insert(key, id);
        Ok(id)
    }

    pub fn write(&mut self, c: Cmd) -> io::Result<()> {
        let mut e = Enc { b: Vec::with_capacity(256) };
        match c {
            Cmd::Upload(Upload::Image { resource, index, format, width, height, depth, pitch, slice, bytes }) => {
                e.u8(0);
                for v in [*resource, *index, *format, *width, *height, *depth, *pitch, *slice] {
                    e.u32(v);
                }
                let id = self.blob(bytes)?;
                e.u32(id);
            }
            Cmd::Upload(Upload::Buffer { resource, bytes }) => {
                e.u8(1);
                e.u32(*resource);
                let id = self.blob(bytes)?;
                e.u32(id);
            }
            Cmd::Draw(d) => {
                e.u8(2);
                self.draw(&mut e, d)?;
            }
            Cmd::Clear(c) => {
                e.u8(3);
                e.u32(c.rects.len() as u32);
                for r in &c.rects {
                    e.rect(r);
                }
                for v in [c.flags, c.colour, c.z.to_bits(), c.stencil] {
                    e.u32(v);
                }
                for t in &c.targets {
                    e.opt_target(t.as_ref());
                }
                e.opt_target(c.depth.as_ref());
                for v in c.viewport {
                    e.u32(v);
                }
                match &c.scissor {
                    Some(r) => {
                        e.u8(1);
                        e.rect(r);
                    }
                    None => e.u8(0),
                }
                e.u8(c.srgb as u8);
            }
            Cmd::Present(t) => {
                e.u8(4);
                e.target(t);
            }
            Cmd::Copy(c) => {
                e.u8(5);
                e.target(&c.src);
                e.rect(&c.src_rect);
                e.target(&c.dst);
                e.rect(&c.dst_rect);
                e.u32(c.filter);
            }
            Cmd::Fill(f) => {
                e.u8(6);
                e.target(&f.dst);
                e.rect(&f.rect);
                e.u32(f.colour);
            }
            Cmd::ReadBack(t) => {
                e.u8(7);
                e.target(t);
            }
            Cmd::QueryBegin(q) => {
                e.u8(8);
                e.u32(q);
            }
            Cmd::QueryEnd(q) => {
                e.u8(9);
                e.u32(q);
            }
            Cmd::QueryResult(q) => {
                e.u8(10);
                e.u32(q);
            }
            Cmd::Gamma(r) => {
                e.u8(11);
                let id = match r {
                    Some(r) => {
                        let bytes: Vec<u8> = r.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
                        self.blob(&bytes)?
                    }
                    None => NONE,
                };
                e.u32(id);
            }
            Cmd::Release(r) => {
                e.u8(12);
                e.u32(r);
            }
        }
        self.record(TAG_COMMAND, &e.b)?;
        self.commands += 1;
        Ok(())
    }

    fn draw(&mut self, e: &mut Enc, d: &DrawCall) -> io::Result<()> {
        e.u32(d.primitive);
        e.u32(d.primitives);
        e.u32(d.start);
        e.u32(d.base_vertex as u32);
        e.u32(d.min_index);
        e.u32(d.vertex_count);
        match &d.vertices {
            Vertices::Streams(s) => {
                e.u8(0);
                e.u32(s.len() as u32);
                for &(n, res, off, stride, freq) in s.iter() {
                    for v in [n, res, off, stride, freq] {
                        e.u32(v);
                    }
                }
            }
            Vertices::Inline { bytes, stride } => {
                e.u8(1);
                let id = self.blob(bytes)?;
                e.u32(id);
                e.u32(*stride);
            }
        }
        match &d.indices {
            Indices::None => e.u8(0),
            Indices::Buffer(res, format) => {
                e.u8(1);
                e.u32(*res);
                e.u32(*format);
            }
            Indices::Inline { bytes, format } => {
                e.u8(2);
                let id = self.blob(bytes)?;
                e.u32(id);
                e.u32(*format);
            }
        }
        for s in [&d.vertex_shader, &d.pixel_shader] {
            let id = match s {
                Some(t) => self.blob(&words(t))?,
                None => NONE,
            };
            e.u32(id);
        }
        let decl: Vec<u8> = d.declaration.iter().flatten().copied().collect();
        let id = self.blob(&decl)?;
        e.u32(id);
        e.u32(d.fvf);
        for c in [&d.vs_constants, &d.ps_constants] {
            let id = self.blob(&constants_bytes(c))?;
            e.u32(id);
        }
        e.u32(d.textures.len() as u32);
        for t in d.textures.iter() {
            for v in [t.sampler, t.resource, kind_code(t.kind), t.format, t.width, t.height, t.depth, t.levels, t.usage] {
                e.u32(v);
            }
            for v in t.states {
                e.u32(v);
            }
        }
        let states: Vec<u8> = d.render_states.iter().flat_map(|&(s, v)| [s, v]).flat_map(u32::to_le_bytes).collect();
        let id = self.blob(&states)?;
        e.u32(id);
        for v in d.viewport {
            e.u32(v);
        }
        e.rect(&d.scissor);
        for t in &d.targets {
            e.opt_target(t.as_ref());
        }
        e.opt_target(d.depth.as_ref());
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    /// The underlying writer (tests write to memory).
    pub fn into_inner(self) -> W {
        self.out
    }
}

fn words(t: &[u32]) -> Vec<u8> {
    t.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn constants_bytes(c: &Constants) -> Vec<u8> {
    let mut b = Vec::with_capacity(4 + c.f.len() * 16 + 320);
    b.extend_from_slice(&(c.f.len() as u32).to_le_bytes());
    for w in c.f.iter().flatten() {
        b.extend_from_slice(&w.to_le_bytes());
    }
    for w in c.i.iter().flatten() {
        b.extend_from_slice(&w.to_le_bytes());
    }
    for w in c.b {
        b.extend_from_slice(&w.to_le_bytes());
    }
    b
}

fn kind_code(k: TexKind) -> u32 {
    match k {
        TexKind::Flat => 0,
        TexKind::Cube => 1,
        TexKind::Volume => 2,
    }
}

struct Enc {
    b: Vec<u8>,
}

impl Enc {
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn rect(&mut self, r: &[i32; 4]) {
        for &v in r {
            self.u32(v as u32);
        }
    }
    fn target(&mut self, t: &Target) {
        for v in [t.surface, t.format, t.width, t.height, t.level, t.samples, t.quality] {
            self.u32(v);
        }
        match &t.texture {
            Some(x) => {
                self.u8(1);
                for v in [x.resource, kind_code(x.kind), x.width, x.height, x.depth, x.levels, x.usage] {
                    self.u32(v);
                }
            }
            None => self.u8(0),
        }
    }
    fn opt_target(&mut self, t: Option<&Target>) {
        match t {
            Some(t) => {
                self.u8(1);
                self.target(t);
            }
            None => self.u8(0),
        }
    }
}

// --- reading -------------------------------------------------------------------

/// Streams a capture file's commands. Blobs are kept as they arrive
/// ([`Reader::open`]), or, from [`Reader::open_indexed`], as their place in
/// the file, inflated when a command uses them (memory bounded by a cache).
pub struct Reader<R: Read = BufReader<std::fs::File>> {
    input: R,
    blobs: Blobs,
    /// the input's position (bytes from the start of the file)
    pos: u64,
}

/// A capture's blobs: in memory, or where they are in the file.
struct Blobs {
    blobs: Vec<Blob>,
    lazy: Option<Lazy>,
}

enum Blob {
    Bytes(std::sync::Arc<[u8]>),
    /// the deflated bytes' offset and length in the file, the inflated length
    At { offset: u64, len: u32, raw: u32 },
}

/// Blobs read on demand from the file, the latest kept up to `cap` bytes.
struct Lazy {
    file: std::fs::File,
    cache: HashMap<usize, std::sync::Arc<[u8]>>,
    order: std::collections::VecDeque<usize>,
    bytes: usize,
    cap: usize,
}

impl Blobs {
    fn get(&mut self, id: usize) -> io::Result<std::sync::Arc<[u8]>> {
        let (offset, len, raw) = match self.blobs.get(id) {
            Some(Blob::Bytes(b)) => return Ok(b.clone()),
            Some(Blob::At { offset, len, raw }) => (*offset, *len, *raw),
            None => return Err(bad(format!("blob {id} used before it was written"))),
        };
        let lazy = self.lazy.as_mut().ok_or_else(|| bad("blob without a file"))?;
        if let Some(b) = lazy.cache.get(&id) {
            return Ok(b.clone());
        }
        use std::io::Seek;
        lazy.file.seek(io::SeekFrom::Start(offset))?;
        let mut deflated = vec![0u8; len as usize];
        lazy.file.read_exact(&mut deflated)?;
        let bytes = miniz_oxide::inflate::decompress_to_vec_with_limit(&deflated, raw as usize).map_err(|e| bad(format!("blob {id}: {e:?}")))?;
        if bytes.len() != raw as usize {
            return Err(bad(format!("blob {id}: {} bytes, expected {raw}", bytes.len())));
        }
        let b: std::sync::Arc<[u8]> = bytes.into();
        lazy.bytes += b.len();
        lazy.cache.insert(id, b.clone());
        lazy.order.push_back(id);
        while lazy.bytes > lazy.cap {
            let Some(old) = lazy.order.pop_front() else { break };
            if let Some(x) = lazy.cache.remove(&old) {
                lazy.bytes -= x.len();
            }
        }
        Ok(b)
    }
}

impl Reader {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Reader> {
        Reader::new(BufReader::with_capacity(1 << 20, std::fs::File::open(path)?))
    }

    /// As [`open`](Self::open), with the blobs left in the file and read
    /// when a command uses them, the latest `cache_bytes` of them kept:
    /// memory stays bounded whatever the capture's length (L113).
    pub fn open_indexed(path: impl AsRef<Path>, cache_bytes: usize) -> io::Result<Reader> {
        let mut r = Reader::open(&path)?;
        r.blobs.lazy = Some(Lazy {
            file: std::fs::File::open(path)?,
            cache: HashMap::new(),
            order: Default::default(),
            bytes: 0,
            cap: cache_bytes,
        });
        Ok(r)
    }
}

fn bad(what: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.into())
}

impl<R: Read> Reader<R> {
    pub fn new(mut input: R) -> io::Result<Reader<R>> {
        let mut head = [0u8; 12];
        input.read_exact(&mut head)?;
        if &head[..8] != MAGIC {
            return Err(bad("not a frame capture (magic)"));
        }
        let version = u32::from_le_bytes(head[8..12].try_into().unwrap());
        if version != VERSION {
            return Err(bad(format!("capture version {version}, this reader reads {VERSION}")));
        }
        Ok(Reader { input, blobs: Blobs { blobs: Vec::new(), lazy: None }, pos: 12 })
    }

    /// The next command; None at the end (a record cut short at the end, as
    /// a session that stopped mid-write leaves, also ends the stream).
    fn next_command(&mut self) -> io::Result<Option<Command>> {
        loop {
            let mut head = [0u8; 5];
            match self.input.read_exact(&mut head) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(e),
            }
            let len = u32::from_le_bytes(head[1..5].try_into().unwrap()) as usize;
            let mut payload = vec![0u8; len];
            match self.input.read_exact(&mut payload) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(e),
            }
            let payload_at = self.pos + 5;
            self.pos += 5 + len as u64;
            match head[0] {
                TAG_BLOB => {
                    if len < 8 {
                        return Err(bad("short blob record"));
                    }
                    let id = u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
                    let raw = u32::from_le_bytes(payload[4..8].try_into().unwrap()) as usize;
                    if id != self.blobs.blobs.len() {
                        return Err(bad(format!("blob {id} out of order")));
                    }
                    if self.blobs.lazy.is_some() {
                        self.blobs.blobs.push(Blob::At { offset: payload_at + 8, len: (len - 8) as u32, raw: raw as u32 });
                        continue;
                    }
                    let bytes = miniz_oxide::inflate::decompress_to_vec_with_limit(&payload[8..], raw).map_err(|e| bad(format!("blob {id}: {e:?}")))?;
                    if bytes.len() != raw {
                        return Err(bad(format!("blob {id}: {} bytes, expected {raw}", bytes.len())));
                    }
                    self.blobs.blobs.push(Blob::Bytes(bytes.into()));
                }
                TAG_COMMAND => {
                    let mut d = Dec { b: &payload, at: 0, blobs: &mut self.blobs };
                    return d.command().map(Some);
                }
                t => return Err(bad(format!("record tag {t}"))),
            }
        }
    }
}

impl<R: Read> Iterator for Reader<R> {
    type Item = io::Result<Command>;
    fn next(&mut self) -> Option<io::Result<Command>> {
        self.next_command().transpose()
    }
}

struct Dec<'a> {
    b: &'a [u8],
    at: usize,
    blobs: &'a mut Blobs,
}

impl Dec<'_> {
    fn u8(&mut self) -> io::Result<u8> {
        let v = *self.b.get(self.at).ok_or_else(|| bad("command cut short"))?;
        self.at += 1;
        Ok(v)
    }
    fn u32(&mut self) -> io::Result<u32> {
        let s = self.b.get(self.at..self.at + 4).ok_or_else(|| bad("command cut short"))?;
        self.at += 4;
        Ok(u32::from_le_bytes(s.try_into().unwrap()))
    }
    fn rect(&mut self) -> io::Result<[i32; 4]> {
        Ok([self.u32()? as i32, self.u32()? as i32, self.u32()? as i32, self.u32()? as i32])
    }
    fn blob(&mut self) -> io::Result<std::sync::Arc<[u8]>> {
        let id = self.u32()? as usize;
        self.blobs.get(id)
    }
    fn opt_blob(&mut self) -> io::Result<Option<std::sync::Arc<[u8]>>> {
        let id = self.u32()?;
        if id == NONE {
            return Ok(None);
        }
        self.blobs.get(id as usize).map(Some)
    }
    fn kind(&mut self) -> io::Result<TexKind> {
        Ok(match self.u32()? {
            0 => TexKind::Flat,
            1 => TexKind::Cube,
            2 => TexKind::Volume,
            k => return Err(bad(format!("texture kind {k}"))),
        })
    }
    fn target(&mut self) -> io::Result<Target> {
        let (surface, format, width, height, level, samples, quality) = (self.u32()?, self.u32()?, self.u32()?, self.u32()?, self.u32()?, self.u32()?, self.u32()?);
        let texture = if self.u8()? != 0 {
            Some(TargetTexture {
                resource: self.u32()?, kind: self.kind()?, width: self.u32()?, height: self.u32()?, depth: self.u32()?, levels: self.u32()?, usage: self.u32()?,
            })
        } else {
            None
        };
        Ok(Target { surface, format, width, height, texture, level, samples, quality })
    }
    fn opt_target(&mut self) -> io::Result<Option<Target>> {
        Ok(if self.u8()? != 0 { Some(self.target()?) } else { None })
    }
    fn targets(&mut self) -> io::Result<[Option<Target>; 4]> {
        Ok([self.opt_target()?, self.opt_target()?, self.opt_target()?, self.opt_target()?])
    }

    fn command(&mut self) -> io::Result<Command> {
        Ok(match self.u8()? {
            0 => {
                let v: Vec<u32> = (0..8).map(|_| self.u32()).collect::<io::Result<_>>()?;
                let bytes = self.blob()?.to_vec();
                Command::Upload(Upload::Image { resource: v[0], index: v[1], format: v[2], width: v[3], height: v[4], depth: v[5], pitch: v[6], slice: v[7], bytes })
            }
            1 => {
                let resource = self.u32()?;
                Command::Upload(Upload::Buffer { resource, bytes: self.blob()?.to_vec() })
            }
            2 => Command::Draw(self.draw()?),
            3 => {
                let n = self.u32()?;
                let rects = (0..n).map(|_| self.rect()).collect::<io::Result<_>>()?;
                let (flags, colour, z, stencil) = (self.u32()?, self.u32()?, f32::from_bits(self.u32()?), self.u32()?);
                let targets = self.targets()?;
                let depth = self.opt_target()?;
                let mut viewport = [0u32; 6];
                for v in &mut viewport {
                    *v = self.u32()?;
                }
                let scissor = if self.u8()? != 0 { Some(self.rect()?) } else { None };
                let srgb = self.u8()? != 0;
                Command::Clear(ClearCall { rects, flags, colour, z, stencil, targets, depth, viewport, scissor, srgb })
            }
            4 => Command::Present(self.target()?),
            5 => Command::Copy(CopyCall { src: self.target()?, src_rect: self.rect()?, dst: self.target()?, dst_rect: self.rect()?, filter: self.u32()? }),
            6 => Command::Fill(FillCall { dst: self.target()?, rect: self.rect()?, colour: self.u32()? }),
            7 => Command::ReadBack(self.target()?),
            8 => Command::QueryBegin(self.u32()?),
            9 => Command::QueryEnd(self.u32()?),
            10 => Command::QueryResult(self.u32()?),
            11 => Command::Gamma(match self.opt_blob()? {
                Some(b) if b.len() == 1536 => {
                    let mut r: GammaRamp = [[0; 256]; 3];
                    for (k, w) in b.chunks_exact(2).enumerate() {
                        r[k / 256][k % 256] = u16::from_le_bytes([w[0], w[1]]);
                    }
                    Some(Box::new(r))
                }
                Some(b) => return Err(bad(format!("gamma ramp of {} bytes", b.len()))),
                None => None,
            }),
            12 => Command::Release(self.u32()?),
            k => return Err(bad(format!("command kind {k}"))),
        })
    }

    fn draw(&mut self) -> io::Result<DrawCall> {
        let (primitive, primitives, start, base_vertex, min_index, vertex_count) =
            (self.u32()?, self.u32()?, self.u32()?, self.u32()? as i32, self.u32()?, self.u32()?);
        let vertices = match self.u8()? {
            0 => {
                let n = self.u32()?;
                let mut v = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    v.push((self.u32()?, self.u32()?, self.u32()?, self.u32()?, self.u32()?));
                }
                Vertices::Streams(v.into())
            }
            1 => Vertices::Inline { bytes: self.blob()?.to_vec(), stride: self.u32()? },
            k => return Err(bad(format!("vertices kind {k}"))),
        };
        let indices = match self.u8()? {
            0 => Indices::None,
            1 => Indices::Buffer(self.u32()?, self.u32()?),
            2 => Indices::Inline { bytes: self.blob()?.to_vec(), format: self.u32()? },
            k => return Err(bad(format!("indices kind {k}"))),
        };
        let tokens = |b: &[u8]| b.chunks_exact(4).map(|w| u32::from_le_bytes(w.try_into().unwrap())).collect::<Vec<u32>>();
        let vertex_shader = self.opt_blob()?.map(|b| tokens(&b).into());
        let pixel_shader = self.opt_blob()?.map(|b| tokens(&b).into());
        let declaration = self.blob()?.chunks_exact(8).map(|e| e.try_into().unwrap()).collect();
        let fvf = self.u32()?;
        let vs_constants = constants_from(&self.blob()?)?;
        let ps_constants = constants_from(&self.blob()?)?;
        let n = self.u32()?;
        let mut textures = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let (sampler, resource, kind, format, width, height, depth, levels, usage) =
                (self.u32()?, self.u32()?, self.kind()?, self.u32()?, self.u32()?, self.u32()?, self.u32()?, self.u32()?, self.u32()?);
            let mut states = [0u32; 14];
            for s in &mut states {
                *s = self.u32()?;
            }
            textures.push(TextureBinding { sampler, resource, kind, format, width, height, depth, levels, usage, states });
        }
        let words = tokens(&self.blob()?);
        let render_states = words.chunks_exact(2).map(|p| (p[0], p[1])).collect::<Vec<_>>().into();
        let mut viewport = [0u32; 6];
        for v in &mut viewport {
            *v = self.u32()?;
        }
        let scissor = self.rect()?;
        let targets = self.targets()?;
        let depth = self.opt_target()?;
        Ok(DrawCall {
            primitive, primitives, start, base_vertex, min_index, vertex_count, vertices, indices, vertex_shader, pixel_shader, declaration, fvf,
            vs_constants: vs_constants.into(), ps_constants: ps_constants.into(), textures: textures.into(), render_states, viewport, scissor, targets, depth,
        })
    }
}

fn constants_from(b: &[u8]) -> io::Result<Constants> {
    let w = |k: usize| -> io::Result<u32> {
        b.get(4 * k..4 * k + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| bad("constant block cut short"))
    };
    let n = w(0)? as usize;
    let mut c = Constants { f: Vec::with_capacity(n), ..Default::default() };
    for k in 0..n {
        c.f.push([w(1 + 4 * k)?, w(2 + 4 * k)?, w(3 + 4 * k)?, w(4 + 4 * k)?]);
    }
    let base = 1 + 4 * n;
    for k in 0..64 {
        c.i[k / 4][k % 4] = w(base + k)? as i32;
    }
    for k in 0..16 {
        c.b[k] = w(base + 64 + k)?;
    }
    Ok(c)
}

// --- capturing ------------------------------------------------------------------

/// A backend that writes every command to a capture file and passes it on
/// to the backend it wraps. Recording stops after `limit` Presents (if
/// set) or at the first write error (kept in `error`); the wrapped backend
/// keeps drawing either way.
pub struct Capture {
    inner: Box<dyn GraphicsBackend>,
    writer: Option<Writer>,
    limit: Option<u32>,
    /// Presents recorded
    pub frames: u32,
    pub error: Option<String>,
}

impl Capture {
    pub fn new(inner: Box<dyn GraphicsBackend>, path: impl AsRef<Path>, limit: Option<u32>) -> io::Result<Capture> {
        Ok(Capture { inner, writer: Some(Writer::create(path)?), limit, frames: 0, error: None })
    }

    /// Wrap `inner` when the environment asks for a capture:
    /// `DDDA_CAPTURE` names the file, `DDDA_CAPTURE_FRAMES` (optional)
    /// stops recording after that many Presents. A file that cannot be
    /// created is reported on stderr and the backend is returned as it is.
    pub fn from_env(inner: Box<dyn GraphicsBackend>) -> Box<dyn GraphicsBackend> {
        let Some(path) = std::env::var_os("DDDA_CAPTURE") else { return inner };
        let limit = std::env::var("DDDA_CAPTURE_FRAMES").ok().and_then(|v| v.parse().ok());
        // the wrapped backend is moved into the capture only once the file
        // exists, so a failure hands it back unchanged
        match Writer::create(&path) {
            Ok(w) => Box::new(Capture { inner, writer: Some(w), limit, frames: 0, error: None }),
            Err(e) => {
                eprintln!("DDDA_CAPTURE {}: {e}", Path::new(&path).display());
                inner
            }
        }
    }

    /// The wrapped backend.
    pub fn inner(&mut self) -> &mut dyn GraphicsBackend {
        self.inner.as_mut()
    }

    fn record(&mut self, c: Cmd) {
        let Some(w) = self.writer.as_mut() else { return };
        let mut r = w.write(c);
        if let (Cmd::Present(_), Ok(())) = (c, &r) {
            self.frames += 1;
            r = w.flush();
            if self.limit.is_some_and(|n| self.frames >= n) {
                self.writer = None;
            }
        }
        if let Err(e) = r {
            self.error = Some(e.to_string());
            self.writer = None;
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(w) = self.writer.as_mut() {
            let _ = w.flush();
        }
    }
}

impl GraphicsBackend for Capture {
    fn fence(&mut self) {
        self.inner.fence();
    }
    fn upload(&mut self, upload: Upload) {
        self.record(Cmd::Upload(&upload));
        self.inner.upload(upload);
    }
    fn draw(&mut self, draw: &DrawCall) {
        self.record(Cmd::Draw(draw));
        self.inner.draw(draw);
    }
    fn clear(&mut self, clear: &ClearCall) {
        self.record(Cmd::Clear(clear));
        self.inner.clear(clear);
    }
    fn present(&mut self, back_buffer: &Target) {
        self.record(Cmd::Present(back_buffer));
        self.inner.present(back_buffer);
    }
    fn copy(&mut self, copy: &CopyCall) {
        self.record(Cmd::Copy(copy));
        self.inner.copy(copy);
    }
    fn fill(&mut self, fill: &FillCall) {
        self.record(Cmd::Fill(fill));
        self.inner.fill(fill);
    }
    fn read_back(&mut self, target: &Target) -> Option<Vec<u8>> {
        self.record(Cmd::ReadBack(target));
        self.inner.read_back(target)
    }
    fn query_begin(&mut self, query: u32) {
        self.record(Cmd::QueryBegin(query));
        self.inner.query_begin(query);
    }
    fn query_end(&mut self, query: u32) {
        self.record(Cmd::QueryEnd(query));
        self.inner.query_end(query);
    }
    fn query_result(&mut self, query: u32) -> Option<u64> {
        self.record(Cmd::QueryResult(query));
        self.inner.query_result(query)
    }
    fn gamma(&mut self, ramp: Option<&GammaRamp>) {
        self.record(Cmd::Gamma(ramp));
        self.inner.gamma(ramp);
    }
    fn release(&mut self, resource: u32) {
        self.record(Cmd::Release(resource));
        self.inner.release(resource);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(surface: u32, texture: Option<TargetTexture>) -> Target {
        Target { surface, format: 21, width: 8, height: 4, texture, level: 1, samples: 4, quality: 0 }
    }

    fn commands() -> Vec<Command> {
        let tex = TargetTexture { resource: 0x300, kind: TexKind::Cube, width: 16, height: 16, depth: 1, levels: 5, usage: 1 };
        let mut vs = Constants { f: vec![[1, 2, 3, 4]; 3], ..Default::default() };
        vs.i[2] = [5, -6, 7, 8];
        vs.b[15] = 1;
        let draw = DrawCall {
            primitive: 4, primitives: 2, start: 3, base_vertex: -5, min_index: 1, vertex_count: 9,
            vertices: Vertices::Streams(vec![(0, 0x500, 16, 24, 1), (1, 0x501, 0, 8, 0x8000_0002)].into()),
            indices: Indices::Buffer(0x600, 101),
            vertex_shader: Some((vec![0xFFFE_0300, 0x0000_FFFF]).into()), pixel_shader: None,
            declaration: vec![[0, 0, 0, 0, 3, 0, 0, 0]].into(), fvf: 0, vs_constants: vs.into(), ps_constants: Constants::default().into(),
            textures: vec![TextureBinding { sampler: 257, resource: 0x300, kind: TexKind::Cube, format: 113, width: 16, height: 16, depth: 1, levels: 5, usage: 1, states: [7; 14] }].into(),
            render_states: vec![(7, 1), (27, 0)].into(), viewport: [0, 0, 8, 4, 0, 0x3F80_0000], scissor: [0, 0, 8, -4],
            targets: [Some(target(0x101, Some(tex))), None, Some(target(0x102, None)), None], depth: Some(target(0x180, None)),
        };
        let mut inline = draw.clone();
        inline.vertices = Vertices::Inline { bytes: vec![1, 2, 3, 4, 5], stride: 5 };
        inline.indices = Indices::Inline { bytes: vec![0, 0, 1, 0], format: 101 };
        let mut ramp: GammaRamp = [[0; 256]; 3];
        ramp[2][255] = 0xFFFF;
        vec![
            Command::Upload(Upload::Image { resource: 0x300, index: 3, format: 21, width: 2, height: 2, depth: 1, pitch: 8, slice: 16, bytes: vec![9; 16] }),
            Command::Upload(Upload::Buffer { resource: 0x500, bytes: vec![9; 16] }),
            Command::Clear(ClearCall {
                rects: vec![[0, 1, 2, 3]], flags: 7, colour: 0x8040_2010, z: 0.25, stencil: 3, targets: [Some(target(0x101, None)), None, None, None],
                depth: None, viewport: [1, 2, 3, 4, 5, 6], scissor: Some([-1, 0, 4, 4]), srgb: true,
            }),
            Command::Draw(draw.clone()),
            Command::Draw(inline),
            Command::Draw(draw),
            Command::Copy(CopyCall { src: target(0x101, None), src_rect: [0; 4], dst: target(0x102, Some(tex)), dst_rect: [1, 1, 2, 2], filter: 2 }),
            Command::Fill(FillCall { dst: target(0x103, None), rect: [0; 4], colour: 0xFF00_FF00 }),
            Command::ReadBack(target(0x103, None)),
            Command::QueryBegin(0x700),
            Command::QueryEnd(0x700),
            Command::QueryResult(0x700),
            Command::Gamma(Some(Box::new(ramp))),
            Command::Gamma(None),
            Command::Present(target(0x100, None)),
            Command::Release(0x300),
        ]
    }

    #[test]
    fn every_command_reads_back_as_written() {
        let cs = commands();
        let mut w = Writer::new(Vec::new()).unwrap();
        for c in &cs {
            w.write(c.as_cmd()).unwrap();
        }
        let blobs = w.next_blob;
        let bytes = w.into_inner();
        let read: Vec<Command> = Reader::new(&bytes[..]).unwrap().collect::<io::Result<_>>().unwrap();
        assert_eq!(read, cs);
        // the repeated draw, the same 16 bytes in an image and a buffer
        // and the empty constant blocks are written once
        assert!(blobs <= 12, "{blobs} blobs");
    }

    /// L113: blobs read from the file when used, through a cache too small
    /// to keep any, give the same commands.
    #[test]
    fn an_indexed_reader_reads_the_same_commands() {
        let cs = commands();
        let path = std::env::temp_dir().join(format!("d3d9frame-indexed-{}.ddframe", std::process::id()));
        let mut w = Writer::create(&path).unwrap();
        for c in &cs {
            w.write(c.as_cmd()).unwrap();
        }
        w.flush().unwrap();
        drop(w);
        for cache in [0, 1 << 20] {
            let read: Vec<Command> = Reader::open_indexed(&path, cache).unwrap().collect::<io::Result<_>>().unwrap();
            assert_eq!(read, cs, "cache {cache}");
        }
        std::fs::remove_file(&path).unwrap();
    }

    struct Null(u32);
    impl GraphicsBackend for Null {
        fn upload(&mut self, _: Upload) {}
        fn draw(&mut self, _: &DrawCall) {}
        fn clear(&mut self, _: &ClearCall) {}
        fn present(&mut self, _: &Target) {
            self.0 += 1;
        }
        fn copy(&mut self, _: &CopyCall) {}
        fn fill(&mut self, _: &FillCall) {}
        fn read_back(&mut self, _: &Target) -> Option<Vec<u8>> {
            None
        }
        fn query_begin(&mut self, _: u32) {}
        fn query_end(&mut self, _: u32) {}
        fn query_result(&mut self, _: u32) -> Option<u64> {
            None
        }
        fn gamma(&mut self, _: Option<&GammaRamp>) {}
        fn release(&mut self, _: u32) {}
    }

    /// DDDA_CAPTURE wraps the backend; DDDA_CAPTURE_FRAMES stops recording
    /// (the wrapped backend still gets every command).
    #[test]
    fn the_environment_switches_capturing_on() {
        let path = std::env::temp_dir().join(format!("ddda-capture-env-{}.ddframe", std::process::id()));
        std::env::set_var("DDDA_CAPTURE", &path);
        std::env::set_var("DDDA_CAPTURE_FRAMES", "2");
        let mut b = Capture::from_env(Box::new(Null(0)));
        std::env::remove_var("DDDA_CAPTURE");
        std::env::remove_var("DDDA_CAPTURE_FRAMES");
        let cs = commands();
        for _ in 0..3 {
            for c in &cs {
                c.apply(b.as_mut());
            }
        }
        let any: &mut dyn std::any::Any = b.as_mut();
        let capture = any.downcast_mut::<Capture>().expect("a capture");
        assert_eq!(capture.frames, 2);
        let any: &mut dyn std::any::Any = capture.inner();
        assert_eq!(any.downcast_mut::<Null>().unwrap().0, 3, "the wrapped backend presented every frame");
        drop(b);
        let read: Vec<Command> = Reader::open(&path).unwrap().collect::<io::Result<_>>().unwrap();
        // two frames: the second list up to its Present (the Release after it is not recorded)
        assert_eq!(read.len(), 2 * cs.len() - 1);
        std::fs::remove_file(&path).unwrap();
        // without the variable the backend is returned as it is
        let b = Capture::from_env(Box::new(Null(0)));
        let any: &dyn std::any::Any = b.as_ref();
        assert!(any.is::<Null>());
    }

    #[test]
    fn a_stream_cut_short_ends_at_the_last_complete_command() {
        let cs = commands();
        let mut w = Writer::new(Vec::new()).unwrap();
        for c in &cs {
            w.write(c.as_cmd()).unwrap();
        }
        let bytes = w.into_inner();
        let read: Vec<Command> = Reader::new(&bytes[..bytes.len() - 3]).unwrap().collect::<io::Result<_>>().unwrap();
        assert_eq!(read, cs[..cs.len() - 1]);
    }
}
