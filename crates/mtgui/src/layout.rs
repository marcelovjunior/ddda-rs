//! Authored GUI geometry, independent of a renderer. This exposes constant
//! object properties; it does not execute animation tracks or sequence opcodes.
use crate::{word, Gui, Property, Result};

#[derive(Clone, Debug)]
pub struct TextureRegion<'a> {
    pub id: u32,
    pub path: &'a str,
    pub rect: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct Object<'a> {
    pub id: u32,
    pub class_hash: u32,
    pub properties: &'a [Property<'a>],
    pub texture_keys: Vec<(u32, u32)>,
    pub vector_keys: Vec<(&'a str, u32, [f32; 4])>,
    pub boolean_keys: Vec<(&'a str, u32, bool)>,
}

impl<'a> Gui<'a> {
    /// cGUIInstAnimControl::vf19 (0x010782a0) and 0x0107ec10:
    /// extra data is a count followed by 20-byte selector/range records.
    pub fn control_range(&self, node: usize, selector: u32) -> Result<Option<[f32; 2]>> {
        let node = self.nodes.get(node).ok_or("node index")?;
        let Some(offset) = node.extra_offset else {
            return Ok(None);
        };
        let data = self.section(0x104).ok_or("extra pool")?.bytes;
        let count = word(data, offset as usize)? as usize;
        let start = offset as usize + 4;
        let end = count
            .checked_mul(20)
            .and_then(|n| n.checked_add(start))
            .ok_or("control span overflow")?;
        for row in data
            .get(start..end)
            .ok_or("control span outside pool")?
            .chunks_exact(20)
        {
            if word(row, 8)? == selector {
                return Ok(Some([
                    f32::from_bits(word(row, 12)?),
                    f32::from_bits(word(row, 16)?),
                ]));
            }
        }
        Ok(None)
    }
    pub fn vector(&self, property: &Property<'_>) -> Option<[f32; 4]> {
        if property.kind != 4 {
            return None;
        }
        let bytes = self.section(0x100)?.bytes;
        let p = property.value_offset as usize;
        let data = bytes.get(p..p.checked_add(16)?)?;
        Some(std::array::from_fn(|i| {
            f32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap())
        }))
    }

    /// Packed atlas corners from rGUI::vf15 (0x01076170), consumed by
    /// cGUIObjTexture::vf12 (0x0106a150), DDDA.exe 2017-12-13 Steam.
    pub fn texture_regions(&self) -> Result<Vec<TextureRegion<'a>>> {
        let mut out = Vec::new();
        for r in self
            .section(0xdc)
            .ok_or("missing texture table")?
            .bytes
            .chunks_exact(48)
        {
            let id = word(r, 0)?;
            let resource = self
                .resources
                .iter()
                .find(|x| x.source_table == 0xdc && x.id == id)
                .ok_or("texture resource missing")?;
            if let Some(path) = resource.path {
                let lo = word(r, 4)?;
                let hi = word(r, 8)?;
                out.push(TextureRegion {
                    id,
                    path,
                    rect: [
                        (lo & 65535) as f32,
                        (lo >> 16) as f32,
                        (hi & 65535) as f32,
                        (hi >> 16) as f32,
                    ],
                });
            }
        }
        Ok(out)
    }

    /// Object range and constant-property byte count: 0x01077ec0, which
    /// constructs the animation's objects from a4 and links their tree.
    pub fn animation_objects(&'a self, name: &str) -> Result<Vec<Object<'a>>> {
        let a = self
            .animations
            .iter()
            .find(|a| a.name == name)
            .ok_or_else(|| format!("animation {name} absent"))?;
        let first = a.words[3] as usize;
        let count = (a.words[1] & 65535) as usize;
        let records = self.section(0xa4).ok_or("missing object table")?.bytes;
        let end = first
            .checked_add(count)
            .and_then(|n| n.checked_mul(32))
            .ok_or("object span overflow")?;
        let records = records.get(first * 32..end).ok_or("object span outside table")?;
        let mut out = Vec::new();
        for r in records.chunks_exact(32) {
            let start = word(r, 20)? as usize;
            let count = (word(r, 4)? & 255) as usize;
            let properties = self
                .properties
                .get(start..start.checked_add(count).ok_or("property span overflow")?)
                .ok_or("object properties outside pool")?;
            let mut texture_keys = Vec::new();
            let mut vector_keys = Vec::new();
            let mut boolean_keys = Vec::new();
            // Sequence spans are packed into a8; b0 uses a 16-bit key count.
            // Fixups and pool selection: rGUI::vf15, 0x01076170.
            let sequences = a.words[1] >> 16;
            let first_sequence = word(r, 24)? as usize;
            let seq = self.section(0xa8).ok_or("missing sequences")?.bytes;
            let tracks = self.section(0xb0).ok_or("missing tracks")?.bytes;
            for s in 0..sequences as usize {
                let p = (first_sequence + s) * 16;
                let n = word(seq, p)? >> 24;
                let first_track = word(seq, p + 12)? as usize;
                for t in 0..n as usize {
                    let t = (first_track + t) * 20;
                    let flags = word(tracks, t)?;
                    if flags & 255 != 7 && flags & 255 != 4 && flags & 255 != 3 {
                        continue;
                    }
                    let name_offset = word(tracks, t + 8)?;
                    let name = crate::text_at(
                        self.section(0xf0).ok_or("missing string pool")?.bytes,
                        name_offset,
                    )?;
                    let key = word(tracks, t + 12)? as usize;
                    let value = word(tracks, t + 16)? as usize;
                    for k in 0..((flags >> 8) & 65535) as usize {
                        let frame = word(
                            self.section(0xf4).ok_or("missing key table")?.bytes,
                            (key + k) * 8,
                        )? & 0xffffff;
                        if flags & 255 == 7 && name == "Texture" {
                            let value = word(
                                self.section(0xfc).ok_or("missing integer pool")?.bytes,
                                value + k * 4,
                            )?;
                            texture_keys.push((frame, value));
                        } else if flags & 255 == 3 {
                            // rGUI::vf15, 0x01076170: kind 3 relocates to
                            // the byte-valued Boolean pool, not the word pool.
                            let values = self.section(0xf8).ok_or("missing boolean pool")?.bytes;
                            let boolean = *values
                                .get(value.checked_add(k).ok_or("boolean key overflow")?)
                                .ok_or("boolean key outside pool")?
                                != 0;
                            boolean_keys.push((name, frame, boolean));
                        } else if flags & 255 == 4 {
                            let values = self.section(0x100).ok_or("missing vector pool")?.bytes;
                            let mut vector = [0.0; 4];
                            for (i, x) in vector.iter_mut().enumerate() {
                                *x = f32::from_bits(word(values, value + k * 16 + i * 4)?);
                            }
                            vector_keys.push((name, frame, vector));
                        }
                    }
                }
            }
            out.push(Object {
                id: word(r, 0)?,
                class_hash: word(r, 16)?,
                properties,
                texture_keys,
                vector_keys,
                boolean_keys,
            });
        }
        Ok(out)
    }
}

impl Object<'_> {
    /// Discrete authored visibility keys (0x01076170 / cGUI animation data).
    /// No interpolation: a Boolean key takes effect at its recorded frame.
    pub fn visible_at(&self, frame: u32) -> bool {
        self.boolean_keys
            .iter()
            .filter(|(name, key, _)| *name == "Visible" && *key <= frame)
            .last()
            .map(|(_, _, value)| *value)
            .unwrap_or_else(|| self.integer("Visible").unwrap_or(1) != 0)
    }
    pub fn property(&self, name: &str) -> Option<&Property<'_>> {
        self.properties.iter().find(|p| p.name == name)
    }
    pub fn scalar(&self, name: &str) -> Option<f32> {
        Some(f32::from_bits(self.property(name)?.word?))
    }
    pub fn integer(&self, name: &str) -> Option<u32> {
        self.property(name)?.word
    }
}

/// Translated from 0x0106a150 (DDDA.exe 2017-12-13 Steam): packed
/// horizontal/vertical ControlPoint selects left/centre/right and top/middle/bottom.
pub fn anchor_offset(size: [f32; 2], control: u32) -> [f32; 2] {
    let axis = |n: f32, v: u32| match v {
        0 => 0.0,
        1 => -n * 0.5,
        2 => -n,
        _ => 0.0,
    };
    [axis(size[0], control & 3), axis(size[1], (control >> 2) & 3)]
}

/// Translated from 0x0067c590 (DDDA.exe 2017-12-13 Steam): both geometry
/// and texture width change, with a three-pixel minimum for positive values.
pub fn gauge_width(full: f32, ratio: f32) -> f32 {
    let width = ratio * full;
    if ratio > 0.0 && width < 3.0 {
        3.0
    } else {
        width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_animation_uses_byte_pool_and_discrete_visibility() {
        let words = |words: &[u32]| words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<_>>();
        let sections = [
            (0xa4, words(&[7, 0, 0, 0, 0, 0, 0, 0])),
            (0xa8, words(&[1 << 24, 0, 0, 0])),
            (0xb0, words(&[(3 << 8) | 3, 0, 0, 0, 1])),
            (0xf0, b"Visible\0".to_vec()),
            (0xf4, words(&[0x07000000, 0, 133, 0, 140, 0])),
            (0xf8, vec![9, 0, 1, 0]),
        ];
        let mut gui = Gui {
            width: 0,
            height: 0,
            root: None,
            nodes: vec![],
            properties: vec![],
            resources: vec![],
            animations: vec![crate::Animation {
                id: 1,
                name: "fixture",
                words: [1, (1 << 16) | 1, 0, 0, 0, 0],
            }],
            sections: sections
                .iter()
                .map(|(header_offset, bytes)| crate::Section {
                    header_offset: *header_offset,
                    offset: 0,
                    count: None,
                    stride: None,
                    bytes,
                })
                .collect(),
        };
        let objects = gui.animation_objects("fixture").unwrap();
        assert_eq!(
            objects[0].boolean_keys,
            [
                ("Visible", 0, false),
                ("Visible", 133, true),
                ("Visible", 140, false)
            ]
        );
        assert!(!objects[0].visible_at(132));
        assert!(objects[0].visible_at(133));
        assert!(objects[0].visible_at(139));
        assert!(!objects[0].visible_at(140));
        drop(objects);
        gui.sections
            .iter_mut()
            .find(|s| s.header_offset == 0xf8)
            .unwrap()
            .bytes = &[9, 0, 1];
        assert_eq!(
            gui.animation_objects("fixture").unwrap_err(),
            "boolean key outside pool"
        );
    }
    #[test]
    fn packed_anchor() {
        assert_eq!(anchor_offset([152.0, 8.0], 4), [0.0, -4.0]);
        assert_eq!(anchor_offset([160.0, 16.0], 6), [-160.0, -8.0]);
        assert_eq!(anchor_offset([12.0, 10.0], 9), [-6.0, -10.0]);
    }
    #[test]
    fn positive_gauge_minimum_and_cropped_width() {
        assert_eq!(gauge_width(152.0, 0.5), 76.0);
        assert_eq!(gauge_width(152.0, 0.001), 3.0);
        assert_eq!(gauge_width(152.0, 0.0), 0.0);
    }
}
