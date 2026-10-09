//! Region file order checked against rRegionStatus::vf10 at 0x00cd9f70.

#[derive(Debug, Clone, PartialEq)]
pub struct RegionStatus {
    pub regions: Vec<Region>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub no: u32,
    pub kind: u32,
    pub elements: Vec<RegionElement>,
}

/// Names match executable reflection; DP/BP semantics and timer units remain
/// unresolved here. HP is per element: do not sum elements to get creature HP.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionElement {
    pub no: u32,
    pub id: u32,
    pub hp_max: f32,
    pub dp_max: f32,
    pub dp_speed: f32,
    pub bp_max: f32,
    pub bp_speed: f32,
    pub damage_adjustment: f32,
    pub hit_stop_adjustment: f32,
    pub surface: u32,
    pub se_surface: u32,
    pub attributes: u32,
    pub dp_reset_timer_max: f32,
    pub bp_reset_timer_max: f32,
}

pub(crate) struct Cursor<'a> {
    pub data: &'a [u8],
    pub at: usize,
}
impl Cursor<'_> {
    pub fn u32(&mut self) -> Result<u32, String> {
        let bytes = self
            .data
            .get(self.at..self.at + 4)
            .ok_or_else(|| format!("truncated scalar at 0x{:x}", self.at))?;
        self.at += 4;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub fn count(&mut self, min_size: usize) -> Result<usize, String> {
        let n = self.u32()? as usize;
        if n > (self.data.len() - self.at) / min_size {
            return Err(format!("count {n} exceeds remaining data at 0x{:x}", self.at));
        }
        Ok(n)
    }
    pub fn finish(&self) -> Result<(), String> {
        if self.at == self.data.len() {
            Ok(())
        } else {
            Err(format!("{} trailing bytes", self.data.len() - self.at))
        }
    }
}

impl RegionStatus {
    /// Translated from 0x00cd9f70 (DDDA.exe 2017-12-13 Steam).
    /// Bounds and full-consumption checks are added for untrusted input.
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        let mut c = Cursor { data, at: 0 };
        if c.u32()? != 0x2011_0930 {
            return Err("unknown region magic".into());
        }
        let count = c.count(12)?;
        let mut regions = Vec::with_capacity(count);
        for _ in 0..count {
            let no = c.u32()?;
            let kind = c.u32()?;
            let count = c.count(56)?;
            let mut elements = Vec::with_capacity(count);
            for _ in 0..count {
                elements.push(RegionElement {
                    no: c.u32()?,
                    id: c.u32()?,
                    hp_max: c.f32()?,
                    dp_max: c.f32()?,
                    dp_speed: c.f32()?,
                    bp_max: c.f32()?,
                    bp_speed: c.f32()?,
                    damage_adjustment: c.f32()?,
                    hit_stop_adjustment: c.f32()?,
                    surface: c.u32()?,
                    se_surface: c.u32()?,
                    attributes: c.u32()?,
                    dp_reset_timer_max: c.f32()?,
                    bp_reset_timer_max: c.f32()?,
                });
            }
            regions.push(Region { no, kind, elements });
        }
        c.finish()?;
        Ok(Self { regions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Vec<u8> {
        let mut words = vec![0x2011_0930, 2, 7, 8, 1, 9, 10];
        words.extend((1..=7).map(|n| (n as f32).to_bits()));
        words.extend([11, 12, 13, 8f32.to_bits(), 9f32.to_bits(), 14, 15, 0]);
        words.into_iter().flat_map(u32::to_le_bytes).collect()
    }
    #[test]
    fn nested_records_keep_disk_order() {
        let s = RegionStatus::parse(&sample()).unwrap();
        assert_eq!(s.regions.len(), 2);
        assert_eq!((s.regions[0].no, s.regions[0].kind), (7, 8));
        assert!(s.regions[1].elements.is_empty());
        assert_eq!(
            s.regions[0].elements[0],
            RegionElement {
                no: 9,
                id: 10,
                hp_max: 1.,
                dp_max: 2.,
                dp_speed: 3.,
                bp_max: 4.,
                bp_speed: 5.,
                damage_adjustment: 6.,
                hit_stop_adjustment: 7.,
                surface: 11,
                se_surface: 12,
                attributes: 13,
                dp_reset_timer_max: 8.,
                bp_reset_timer_max: 9.,
            }
        );
    }
    #[test]
    fn rejects_truncation_counts_and_trailing_bytes() {
        let b = sample();
        for len in 0..b.len() {
            assert!(RegionStatus::parse(&b[..len]).is_err());
        }
        let mut bad = b.clone();
        bad[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(RegionStatus::parse(&bad).is_err());
        let mut bad = b;
        bad.push(0);
        assert!(RegionStatus::parse(&bad).is_err());
    }
}
