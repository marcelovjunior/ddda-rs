//! Read-only rGUI v0x21209 memory-image tables. Unknown fields remain raw.
//! Layout and fixups: DDDA.exe 2017-12-13 Steam, 0x01076170.
use std::ops::Range;
pub type Result<T> = std::result::Result<T, String>;
const NONE: u32 = u32::MAX;

#[derive(Debug, Clone)]
pub struct Section<'a> {
    pub header_offset: usize,
    pub offset: usize,
    /// None for pools and reflection-dependent records.
    pub stride: Option<usize>,
    pub count: Option<usize>,
    pub bytes: &'a [u8],
}
#[derive(Debug, Clone)]
pub struct Property<'a> {
    pub kind: u8,
    pub flags: u32,
    pub name: &'a str,
    pub value_offset: u32,
    /// Raw first word for scalar kinds; no inferred semantics.
    pub word: Option<u32>,
    pub string: Option<&'a str>,
}
#[derive(Debug, Clone)]
pub struct Node {
    pub id: u32,
    pub unknown: u32,
    pub next_sibling: Option<usize>,
    pub first_child: Option<usize>,
    pub class_hash: u32,
    pub properties: Range<usize>,
    pub extra_offset: Option<u32>,
}
#[derive(Debug, Clone)]
pub struct Animation<'a> {
    pub id: u32,
    pub name: &'a str,
    pub words: [u32; 6],
}
#[derive(Debug, Clone)]
pub struct Resource<'a> {
    pub class: &'static str,
    pub id: u32,
    pub path: Option<&'a str>,
    pub source_table: usize,
}
#[derive(Debug, Clone)]
pub struct Gui<'a> {
    pub width: u32,
    pub height: u32,
    pub root: Option<usize>,
    pub sections: Vec<Section<'a>>,
    pub nodes: Vec<Node>,
    pub animations: Vec<Animation<'a>>,
    pub properties: Vec<Property<'a>>,
    pub resources: Vec<Resource<'a>>,
}
fn word(b: &[u8], p: usize) -> Result<u32> {
    let v = b
        .get(p..p.checked_add(4).ok_or("offset overflow")?)
        .ok_or_else(|| format!("word outside data at {p:#x}"))?;
    Ok(u32::from_le_bytes(v.try_into().unwrap()))
}
fn index(v: u32, count: usize) -> Result<Option<usize>> {
    if v == NONE {
        Ok(None)
    } else if (v as usize) < count {
        Ok(Some(v as usize))
    } else {
        Err(format!("index {v} outside {count}"))
    }
}
fn span(start: u32, count: u32, len: usize) -> Result<Range<usize>> {
    let start = start as usize;
    let end = start.checked_add(count as usize).ok_or("span overflow")?;
    if end > len {
        return Err(format!("span {start}..{end} outside {len}"));
    }
    Ok(start..end)
}
fn text_at(b: &[u8], p: u32) -> Result<&str> {
    let tail = b.get(p as usize..).ok_or("string offset outside pool")?;
    let n = tail.iter().position(|&v| v == 0).ok_or("unterminated string")?;
    std::str::from_utf8(&tail[..n]).map_err(|e| format!("string: {e}"))
}
// (pointer header offset, count header offset, stride). From relocation loops.
const TABLES: &[(usize, usize, usize)] = &[
    (0x9c, 0x28, 24),
    (0xa0, 0x2c, 12),
    (0xa4, 0x30, 32),
    (0xa8, 0x34, 16),
    (0xac, 0x38, 12),
    (0xb0, 0x3c, 20),
    (0xb4, 0x44, 32),
    (0xb8, 0x48, 16),
    (0xbc, 0x4c, 36),
    (0xc0, 0x50, 8),
    (0xc4, 0x54, 8),
    (0xc8, 0x58, 8),
    (0xcc, 0x5c, 16),
    (0xd0, 0x60, 4),
    (0xd4, 0x64, 20),
    (0xd8, 0x68, 12),
    (0xdc, 0x6c, 48),
    (0xe0, 0x70, 12),
    (0xe8, 0x78, 12),
    (0xec, 0x7c, 12),
    (0xf4, 0x40, 8),
];
impl<'a> Gui<'a> {
    // Translated from 0x01075780 / 0x01076170 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(b: &'a [u8]) -> Result<Self> {
        if b.len() < 0x110 || b.get(..4) != Some(b"GUI\0") {
            return Err("not a GUI header".into());
        }
        if word(b, 4)? != 0x21209 {
            return Err("unsupported GUI version".into());
        }
        if word(b, 8)? as usize != b.len() {
            return Err("GUI declared size mismatch".into());
        }
        // Validate all pointers before any pool slice uses the next pointer.
        for h in (0x9c..=0x104).step_by(4).chain(std::iter::once(0x10c)) {
            let offset = word(b, h)? as usize;
            if offset > b.len() || (offset != 0 && offset < 0x110) {
                return Err(format!("section {h:#x} offset {offset:#x}"));
            }
        }
        let mut sections = Vec::new();
        for h in (0x9c..=0x104).step_by(4).chain(std::iter::once(0x10c)) {
            let offset = word(b, h)? as usize;
            if offset > b.len() || (offset != 0 && offset < 0x110) {
                return Err(format!("section {h:#x} offset {offset:#x}"));
            }
            let end = (0x9c..=0x104)
                .step_by(4)
                .chain(std::iter::once(0x10c))
                .map(|p| word(b, p).unwrap() as usize)
                .filter(|&o| o > offset)
                .min()
                .unwrap_or(b.len());
            let layout = TABLES.iter().find(|&&(p, _, _)| p == h);
            let (count, stride) = match layout {
                Some(&(_, c, s)) => (Some(word(b, c)? as usize), Some(s)),
                None => (None, None),
            };
            let length = match (count, stride) {
                (Some(c), Some(s)) => c.checked_mul(s).ok_or("table size overflow")?,
                _ if h == 0x10c => word(b, 0x84)? as usize,
                _ => end.saturating_sub(offset),
            };
            if (offset == 0 && length != 0) || length > end.saturating_sub(offset) {
                return Err(format!("section {h:#x} exceeds next section"));
            }
            sections.push(Section {
                header_offset: h,
                offset,
                count,
                stride,
                bytes: &b[offset..offset + length],
            });
        }
        let section = |h| sections.iter().find(|s| s.header_offset == h).unwrap().bytes;
        let strings = section(0xf0);
        let mut properties = Vec::new();
        for p in section(0xac).chunks_exact(12) {
            let flags = word(p, 0)?;
            let kind = flags as u8;
            let value_offset = word(p, 8)?;
            let name = text_at(strings, word(p, 4)?)?;
            let (w, string) = match kind {
                3 => (
                    Some(
                        *section(0xf8)
                            .get(value_offset as usize)
                            .ok_or("boolean outside pool")? as u32,
                    ),
                    None,
                ),
                6 => {
                    let o = word(section(0xfc), value_offset as usize)?;
                    (Some(o), Some(text_at(strings, o)?))
                }
                1 | 2 | 7 | 8 | 9 | 10 | 11 | 12 | 13 | 14 | 16 => {
                    (Some(word(section(0xfc), value_offset as usize)?), None)
                }
                4 => {
                    section(0x100)
                        .get(
                            value_offset as usize
                                ..(value_offset as usize).checked_add(16).ok_or("vector overflow")?,
                        )
                        .ok_or("vector outside pool")?;
                    (None, None)
                }
                _ => (None, None),
            };
            properties.push(Property {
                kind,
                flags,
                name,
                value_offset,
                word: w,
                string,
            });
        }
        let node_count = word(b, 0x44)? as usize;
        let mut nodes = Vec::new();
        for p in section(0xb4).chunks_exact(32) {
            let extra = word(p, 28)?;
            if extra != NONE && extra as usize >= section(0x104).len() {
                return Err("node extra outside pool".into());
            }
            nodes.push(Node {
                id: word(p, 0)?,
                unknown: word(p, 4)?,
                next_sibling: index(word(p, 8)?, node_count)?,
                first_child: index(word(p, 12)?, node_count)?,
                class_hash: word(p, 20)?,
                properties: span(word(p, 24)?, word(p, 16)?, properties.len())?,
                extra_offset: (extra != NONE).then_some(extra),
            });
        }
        let root = if nodes.is_empty() {
            None
        } else {
            index(word(b, 0x24)?, nodes.len())?
        };
        let mut animations = Vec::new();
        for p in section(0x9c).chunks_exact(24) {
            let words = [
                word(p, 0)?,
                word(p, 4)?,
                word(p, 8)?,
                word(p, 12)?,
                word(p, 16)?,
                word(p, 20)?,
            ];
            if words[5] as usize > section(0xa0).len() / 12 {
                return Err("animation sequence index".into());
            }
            animations.push(Animation {
                id: words[0],
                name: text_at(strings, words[4])?,
                words,
            });
        }
        let mut resources = Vec::new();
        for (h, stride, path_at, class) in [
            (0xdc, 48, 40, "rTexture"),
            (0xe0, 12, 8, "rGUIFont"),
            (0xe8, 12, 8, "rGUIMessage"),
            (0xec, 12, 8, "rGUI"),
        ] {
            for p in section(h).chunks_exact(stride) {
                let off = word(p, path_at)?;
                resources.push(Resource {
                    class,
                    id: word(p, 0)?,
                    path: if off == NONE {
                        None
                    } else {
                        Some(text_at(strings, off)?)
                    },
                    source_table: h,
                });
            }
        }
        // Translated from 0x01075c20 and reflected layouts 0x01153810,
        // 0x01153a60, 0x01153de0 (DDDA.exe 2017-12-13 Steam).
        let filters = section(0xe4);
        let mut pos = 0;
        for _ in 0..word(b, 0x74)? {
            let class = word(filters, pos)?;
            let id = word(filters, pos + 4)?;
            let size = match class {
                0 => 8,
                0x5f704244 | 0x6df3ab1e => 20,
                0x77214222 => {
                    let resource_class = word(filters, pos + 8)?;
                    let off = word(filters, pos + 12)?;
                    if resource_class != NONE && off != NONE {
                        if resource_class != 0x241f5deb {
                            return Err("unexpected filter resource class".into());
                        }
                        resources.push(Resource {
                            class: "rTexture",
                            id,
                            path: Some(text_at(strings, off)?),
                            source_table: 0xe4,
                        });
                    }
                    16
                }
                _ => return Err(format!("unknown reflection filter {class:#x}")),
            };
            pos = pos.checked_add(size).ok_or("filter overflow")?;
            if pos > filters.len() {
                return Err("truncated filter".into());
            }
        }
        let gui = Self {
            width: word(b, 0x90)?,
            height: word(b, 0x94)?,
            root,
            sections,
            nodes,
            animations,
            properties,
            resources,
        };
        gui.preorder()?;
        Ok(gui)
    }
    pub fn section(&self, header_offset: usize) -> Option<&Section<'a>> {
        self.sections.iter().find(|s| s.header_offset == header_offset)
    }
    /// The node graph has a synthetic parent above the root sibling list.
    /// Translated from 0x00e0b840 / 0x010777a0 (DDDA.exe 2017-12-13 Steam).
    /// Reject cycles, duplicate ownership and unreachable nodes; never recurse.
    pub fn preorder(&self) -> Result<Vec<(usize, usize)>> {
        let mut seen = vec![false; self.nodes.len()];
        let mut stack = Vec::new();
        let mut out = Vec::new();
        if let Some(root) = self.root {
            stack.push((root, 0));
        }
        while let Some((i, depth)) = stack.pop() {
            let node = self.nodes.get(i).ok_or("node index outside table")?;
            if seen[i] {
                return Err("node cycle or multiple parents".into());
            }
            seen[i] = true;
            out.push((i, depth));
            if let Some(s) = node.next_sibling {
                stack.push((s, depth));
            }
            if let Some(c) = node.first_child {
                stack.push((c, depth + 1));
            }
        }
        if seen.iter().any(|s| !s) {
            return Err("unreachable instance node".into());
        }
        Ok(out)
    }
    pub fn node_property(&self, node: usize, name: &str) -> Option<&Property<'a>> {
        self.properties
            .get(self.nodes.get(node)?.properties.clone())?
            .iter()
            .find(|p| p.name == name)
    }
    /// Only local animation names: ResourceId -1 selects this GUI.
    /// Translated from 0x0107a8f0 (DDDA.exe 2017-12-13 Steam).
    pub fn local_animation(&self, node: usize) -> Option<&Animation<'a>> {
        if self.node_property(node, "ResourceId")?.word? != NONE {
            return None;
        }
        let id = self.node_property(node, "AnimationId")?.word?;
        self.animations.iter().find(|a| a.id == id)
    }
}
/// Diagnostic class names, hashes verified against the DTI registry.
pub fn node_class_name(hash: u32) -> Option<&'static str> {
    match hash {
        0x419b2827 => Some("cGUIInstNull"),
        0x1daa79db => Some("cGUIInstAnimation"),
        0x723d0214 => Some("cGUIInstAnimVariable"),
        0x7c431e8a => Some("cGUIInstAnimControl"),
        0x010722fe => Some("cGUIInstMessage"),
        0x5fdb73af => Some("cGUIInstText"),
        _ => None,
    }
}
