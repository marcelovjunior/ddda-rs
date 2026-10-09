//! Read-only DDDA effect provider links and effect-list resource references.
//! Particle bodies are deliberately opaque; no effect simulation is implemented.

pub type Result<T> = std::result::Result<T, String>;
fn span(b: &[u8], p: usize, n: usize) -> Result<&[u8]> {
    b.get(p..p.checked_add(n).ok_or("offset overflow")?)
        .ok_or_else(|| format!("out of bounds at {p:#x}, size {n:#x}"))
}
fn word(b: &[u8], p: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(span(b, p, 4)?.try_into().unwrap()))
}
fn half(b: &[u8], p: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(span(b, p, 2)?.try_into().unwrap()))
}
fn string(b: &[u8], p: usize, limit: usize) -> Result<String> {
    let rest = span(b, p, limit)?;
    let n = rest
        .iter()
        .position(|&x| x == 0)
        .ok_or_else(|| format!("unterminated string at {p:#x}"))?;
    String::from_utf8(rest[..n].to_vec()).map_err(|e| e.to_string())
}
struct Cursor<'a> {
    b: &'a [u8],
    p: usize,
}
impl Cursor<'_> {
    fn word(&mut self) -> Result<u32> {
        let v = word(self.b, self.p)?;
        self.p += 4;
        Ok(v)
    }
    fn count(&mut self, min: usize) -> Result<usize> {
        let n = self.word()? as usize;
        if n > self.b.len().saturating_sub(self.p) / min {
            return Err("count exceeds remaining file".into());
        }
        Ok(n)
    }
    fn path(&mut self) -> Result<String> {
        let s = string(self.b, self.p, self.b.len() - self.p)?;
        self.p += s.len() + 1;
        Ok(s)
    }
    fn words<const N: usize>(&mut self) -> Result<[u32; N]> {
        let mut a = [0; N];
        for x in &mut a {
            *x = self.word()?
        }
        Ok(a)
    }
}
#[derive(Debug, Clone)]
pub struct Element {
    pub lists: [String; 8],
    pub effect_2d: String,
    /// 69 serialized words following the nine paths. Float/flag/LOD interpretation
    /// is retained losslessly; these are not runtime-object offsets.
    pub parameters: [u32; 69],
}
#[derive(Debug, Clone)]
pub struct MotionLink {
    pub motion: u32,
    pub selector: u32,
    pub start_frame: f32,
    pub end_frame: f32,
    pub frame_parameter: f32,
    pub flags: [u32; 5],
    pub index: u32,
    pub element: u32,
}
#[derive(Debug, Clone)]
pub struct EventLink {
    pub event: u32,
    pub index: u32,
    pub element: u32,
}
#[derive(Debug, Clone)]
pub struct Provider {
    pub indices: Vec<Vec<Element>>,
    pub motions: Vec<MotionLink>,
    pub events: Vec<EventLink>,
}
impl Provider {
    /// Translated from 0x00cb6cf0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &[u8]) -> Result<Self> {
        if span(b, 0, 4)? != b"epv\0" || word(b, 4)? != 0 {
            return Err("unsupported EPV header".into());
        }
        let mut c = Cursor { b, p: 8 };
        let n = c.count(4)?;
        let mut indices = Vec::new();
        for _ in 0..n {
            let n = c.count(285)?;
            let mut elements = Vec::new();
            for _ in 0..n {
                let mut lists = std::array::from_fn(|_| String::new());
                for p in &mut lists {
                    *p = c.path()?
                }
                let effect_2d = c.path()?;
                let parameters = c.words()?;
                elements.push(Element {
                    lists,
                    effect_2d,
                    parameters,
                });
            }
            indices.push(elements)
        }
        let n = c.count(48)?;
        let mut motions = Vec::new();
        for _ in 0..n {
            let w = c.words::<12>()?;
            motions.push(MotionLink {
                motion: w[0],
                selector: w[1],
                start_frame: f32::from_bits(w[2]),
                end_frame: f32::from_bits(w[3]),
                frame_parameter: f32::from_bits(w[4]),
                flags: w[5..10].try_into().unwrap(),
                index: w[10],
                element: w[11],
            });
        }
        let n = c.count(12)?;
        let mut events = Vec::new();
        for _ in 0..n {
            events.push(EventLink {
                event: c.word()?,
                index: c.word()?,
                element: c.word()?,
            })
        }
        if c.p != b.len() {
            return Err(format!("EPV trailing bytes at {:#x}/{}", c.p, b.len()));
        }
        let result = Self {
            indices,
            motions,
            events,
        };
        for m in &result.motions {
            if result.element(m.index, m.element).is_none() {
                return Err(format!("invalid motion target {}/{}", m.index, m.element));
            }
        }
        for e in &result.events {
            if result.element(e.index, e.element).is_none() {
                return Err(format!("invalid event target {}/{}", e.index, e.element));
            }
        }
        Ok(result)
    }
    pub fn element(&self, index: u32, element: u32) -> Option<&Element> {
        self.indices.get(index as usize)?.get(element as usize)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub class: &'static str,
    pub path: String,
}
#[derive(Debug, Clone)]
pub struct Unit {
    pub generator_kind: u8,
    pub particle_kind: u8,
    pub third_kind: u8,
    pub life_kind: u8,
    pub offsets: [u32; 4],
}
#[derive(Debug, Clone)]
pub struct EffectList {
    pub id: u32,
    pub auxiliary_count: u16,
    pub flags: u32,
    pub units: Vec<Unit>,
    pub resources: Vec<Resource>,
    /// Retained bytes include unresolved particle/joint/movement bodies.
    pub payload: Vec<u8>,
}
fn resource(b: &[u8], p: usize, max: usize, class: &'static str, out: &mut Vec<Resource>) -> Result<()> {
    let path = string(b, p, max)?;
    if !path.is_empty() {
        out.push(Resource { class, path })
    }
    Ok(())
}
fn relative_resource(
    b: &[u8],
    base: usize,
    field: usize,
    class: &'static str,
    out: &mut Vec<Resource>,
) -> Result<()> {
    let off = half(b, base + field)? as usize;
    if off != 0 {
        resource(b, base + off, b.len().saturating_sub(base + off), class, out)?
    }
    Ok(())
}
fn generator(b: &[u8], p: usize, out: &mut Vec<Resource>) -> Result<()> {
    span(b, p, 0xbe)?;
    for (o, c) in [
        (0xb8, "rEffectStrip"),
        (0xba, "rVibration"),
        (0xbc, "rSoundRequest"),
    ] {
        relative_resource(b, p, o, c, out)?
    }
    Ok(())
}
fn life(b: &[u8], p: usize, kind: u8, out: &mut Vec<Resource>) -> Result<()> {
    span(b, p, 8)?;
    if kind == 3 {
        resource(b, p + 0x80, 64, "rEffectStrip", out)?
    }
    if kind <= 8 {
        let off = half(b, p + 6)? as usize;
        if off != 0 {
            let q = p + off;
            span(b, q, 32)?;
            relative_resource(b, q, 0x1c, "rEffectList", out)?;
            relative_resource(b, q, 0x1e, "rEffectList", out)?
        }
    }
    Ok(())
}
fn particle(b: &[u8], p: usize, kind: u8, out: &mut Vec<Resource>) -> Result<()> {
    span(b, p, 4)?;
    match kind {
        0 | 1 | 2 | 3 | 12 | 13 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 23 => {
            for off in [0x70, 0xb0, 0xf0] {
                resource(b, p + off, 64, "rTexture", out)?
            }
            resource(b, p + 0x130, 64, "rEffectAnim", out)?;
        }
        5 => {
            resource(b, p + 0x50, 64, "rModel", out)?;
            relative_resource(b, p, 0x138, "rEffectAnim", out)?;
        }
        6 => {
            if [1, 3, 5].contains(&(word(b, p + 0x170)? as u8 & 15)) {
                for off in [0x70, 0xb0, 0xf0] {
                    resource(b, p + off, 64, "rTexture", out)?
                }
                resource(b, p + 0x130, 64, "rEffectAnim", out)?
            }
        }
        7 => {
            resource(b, p + 0xa0, 64, "rEffectAnim", out)?;
            resource(b, p + 0xe0, 64, "rTexture", out)?;
        }
        8 | 25 => {
            resource(b, p + 0x60, 64, "rEffectAnim", out)?;
            resource(b, p + 0xa0, 64, "rTexture", out)?;
        }
        9 => {
            span(b, p + 0x40, 1)?;
            if b[p + 0x40] == 0 {
                resource(b, p + 0xa0, 64, "rTexture", out)?
            }
        }
        24 => resource(b, p + 0x40, 64, "rGrassWind", out)?,
        _ => {}
    }
    Ok(())
}
impl EffectList {
    /// Translated resource traversal from 0x01147790, 0x01147400,
    /// 0x01146b30, 0x01146bf0, 0x01146e80 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &[u8]) -> Result<Self> {
        if span(b, 0, 4)? != b"EFL\0" || word(b, 4)? != 0x20110318 {
            return Err("unsupported EFL header".into());
        }
        span(b, 0, 48)?;
        let size = word(b, 8)? as usize;
        if size != b.len() - 48 {
            return Err(format!("EFL payload size {size} != {}", b.len() - 48));
        }
        let d = &b[48..];
        let n = half(b, 16)? as usize;
        span(d, 0, n * 16)?;
        let auxiliary_count = half(b, 18)?;
        let flags = word(b, 20)?;
        let mut units = Vec::new();
        let mut resources = Vec::new();
        for i in 0..n {
            let mut offsets = [0; 4];
            let mut kinds = [0; 4];
            for j in 0..4 {
                let w = word(d, i * 16 + j * 4)?;
                offsets[j] = w >> 8;
                kinds[j] = w as u8;
                if offsets[j] != 0 {
                    span(d, offsets[j] as usize, 1)?;
                }
            }
            if offsets[0] != 0 {
                generator(d, offsets[0] as usize, &mut resources)?
            }
            if offsets[1] != 0 {
                particle(d, offsets[1] as usize, kinds[1], &mut resources)?
            }
            if offsets[3] != 0 {
                life(d, offsets[3] as usize, kinds[3] & 15, &mut resources)?
            }
            units.push(Unit {
                generator_kind: kinds[0],
                particle_kind: kinds[1],
                third_kind: kinds[2],
                life_kind: kinds[3] & 15,
                offsets,
            });
        }
        for o in [32, 36, 40, 44] {
            let p = word(b, o)? as usize;
            if p != 0 {
                span(d, p, 1)?;
            }
        }
        if flags & 15 != 0 {
            let g = word(b, 32)? as usize;
            if g != 0 {
                generator(d, g, &mut resources)?
            }
            let l = word(b, 36)? as usize;
            if l != 0 {
                life(d, l, ((flags >> 4) & 15) as u8, &mut resources)?
            }
        }
        let p = word(b, 44)? as usize;
        if p != 0 {
            relative_resource(d, p, 10, "rEffectList", &mut resources)?
        }
        Ok(Self {
            id: word(b, 12)?,
            auxiliary_count,
            flags,
            units,
            resources,
            payload: d.to_vec(),
        })
    }
}
