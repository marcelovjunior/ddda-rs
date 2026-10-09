//! Installed bitmap GUI fonts (GFD 0x10705). No glyph pixels are embedded.
use crate::{word, Result};

#[derive(Clone, Debug)]
pub struct Glyph {
    pub character: u32,
    pub page: u8,
    pub baseline: usize,
    pub advance: u16,
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    pub bearing: u8,
}

pub struct Font<'a> {
    pub texture_base: &'a str,
    pub pages: u32,
    pub nominal_size: u32,
    pub texture_suffix: u32,
    pub baselines: Vec<f32>,
    pub glyphs: Vec<Glyph>,
}

impl<'a> Font<'a> {
    /// Translated from rGUIFont::vf10, 0x00fff120 (2017-12-13 Steam).
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.get(..4) != Some(b"GFD\0") || word(bytes, 4)? != 0x10705 {
            return Err("unsupported GFD font".into());
        }
        let pages = word(bytes, 24)?;
        let glyph_count = word(bytes, 28)? as usize;
        let baseline_count = word(bytes, 32)? as usize;
        let p = baseline_count
            .checked_mul(4)
            .and_then(|n| n.checked_add(44))
            .ok_or("font baseline overflow")?;
        let mut baselines = Vec::new();
        let baseline_data = bytes.get(44..p).ok_or("font baselines truncated")?;
        for value in baseline_data.chunks_exact(4) {
            baselines.push(f32::from_le_bytes(value.try_into().unwrap()));
        }
        let length = word(bytes, p)? as usize;
        let start = p + 4;
        let end = start.checked_add(length).ok_or("font string overflow")?;
        let texture_base = std::str::from_utf8(bytes.get(start..end).ok_or("font path truncated")?)
            .map_err(|e| e.to_string())?;
        if bytes.get(end) != Some(&0) {
            return Err("font path missing terminator".into());
        }
        let start = end + 1;
        let end = glyph_count
            .checked_mul(16)
            .and_then(|n| n.checked_add(start))
            .ok_or("font glyph overflow")?;
        let mut glyphs = Vec::new();
        // Glyph rectangle fields: cGUIInstMessage::vf29, 0x010ff260.
        // Advance, bearing and baseline group: 0x010018f0.
        for row in bytes
            .get(start..end)
            .ok_or("font glyphs truncated")?
            .chunks_exact(16)
        {
            let u16at = |p| u16::from_le_bytes([row[p], row[p + 1]]);
            let packed = word(row, 8)?;
            let glyph = Glyph {
                character: word(row, 12)?,
                page: row[0],
                baseline: (row[1] & 15) as usize,
                advance: u16at(2),
                x: u16at(4),
                y: u16at(6),
                width: (packed & 4095) as u16,
                height: ((packed >> 12) & 4095) as u16,
                bearing: row[11],
            };
            if u32::from(glyph.page) >= pages {
                return Err("font page index".into());
            }
            if glyphs
                .last()
                .is_some_and(|g: &Glyph| g.character >= glyph.character)
            {
                return Err("font glyphs not sorted".into());
            }
            glyphs.push(glyph);
        }
        let nominal_size = word(bytes, 20)?;
        if nominal_size == 0 {
            return Err("zero nominal font size".into());
        }
        Ok(Self {
            texture_base,
            pages,
            nominal_size,
            texture_suffix: word(bytes, 12)?,
            baselines,
            glyphs,
        })
    }
    /// Source binary search: 0x00fff070; unsupported characters stay explicit.
    pub fn glyph(&self, c: char) -> Option<&Glyph> {
        self.glyphs
            .binary_search_by_key(&(c as u32), |g| g.character)
            .ok()
            .map(|i| &self.glyphs[i])
    }
    pub fn texture_path(&self, page: u8) -> Result<String> {
        // Short resource-name suffix table at 0x016307b8, used by 0x00fff120.
        let suffix = match self.texture_suffix {
            0 | 4..=7 => "",
            1 => "_ID",
            2 => "_HQ",
            3 => "_ID_HQ",
            _ => return Err("font texture suffix".into()),
        };
        Ok(format!("{}_{page:02}{suffix}", self.texture_base))
    }
    /// Plain, single-line proportional text only. Layout markup, ruby,
    /// wrapping and font filters are intentionally outside this function.
    /// Translated from 0x010018f0 (DDDA.exe 2017-12-13 Steam).
    pub fn line(&self, text: &str, size: [f32; 2]) -> Result<Vec<PlacedGlyph<'_>>> {
        let sx = size[0] / self.nominal_size as f32;
        let sy = size[1] / self.nominal_size as f32;
        let mut x = 0.0;
        let mut out = Vec::new();
        for c in text.chars() {
            let glyph = self.glyph(c).ok_or_else(|| format!("font missing {c:?}"))?;
            let baseline = self.baselines.get(glyph.baseline).copied().unwrap_or(0.0);
            out.push(PlacedGlyph {
                glyph,
                position: [x - glyph.bearing as f32 * sx, baseline * sy],
                size: [glyph.width as f32 * sx, glyph.height as f32 * sy],
            });
            x += glyph.advance as f32 * sx;
        }
        Ok(out)
    }
}
pub struct PlacedGlyph<'a> {
    pub glyph: &'a Glyph,
    pub position: [f32; 2],
    pub size: [f32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proportional_metrics_and_bearing() {
        let font = Font {
            texture_base: "synthetic",
            pages: 1,
            nominal_size: 20,
            texture_suffix: 0,
            baselines: vec![4.0],
            glyphs: vec![Glyph {
                character: 65,
                page: 0,
                baseline: 0,
                advance: 10,
                x: 0,
                y: 0,
                width: 12,
                height: 20,
                bearing: 2,
            }],
        };
        let line = font.line("AA", [10.0, 10.0]).unwrap();
        assert_eq!(line[0].position, [-1.0, 2.0]);
        assert_eq!(line[1].position, [4.0, 2.0]);
        assert_eq!(line[0].size, [6.0, 10.0]);
        assert!(font.line("B", [10.0, 10.0]).is_err());
    }
    #[test]
    fn truncated_font_is_an_error() {
        for n in 0..44 {
            assert!(Font::parse(&vec![0; n]).is_err());
        }
    }
}
