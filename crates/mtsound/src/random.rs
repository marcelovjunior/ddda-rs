//! Installed sound-random tables and one source RNG draw at a time.
use crate::{header, table, u32at};

#[derive(Debug, Clone)]
pub struct RandomRow {
    pub id: u32,
    pub choices: [(i32, u32); 16],
    pub allow_repeat: bool,
    pub previous: i32,
}

#[derive(Debug, Clone)]
pub struct RandomRequests(pub Vec<RandomRow>);

impl RandomRequests {
    /// Translated from rSoundRandom::load 0x00e30370
    /// (DDDA.exe 2017-12-13 Steam). Runtime rows share the file layout.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        header(bytes, b"DNRS", 2, 16)?;
        let rows = table(bytes, 16, u32at(bytes, 8)? as usize, 0x8c)?;
        let mut result = Vec::new();
        for row in rows.chunks_exact(0x8c) {
            let mut choices = [(0, 0); 16];
            for (index, choice) in choices.iter_mut().enumerate() {
                *choice = (u32at(row, 4 + index * 8)? as i32,
                    u32at(row, 8 + index * 8)?);
            }
            result.push(RandomRow { id: u32at(row, 0)?, choices,
                allow_repeat: row[0x84] == 1, previous: u32at(row, 0x88)? as i32 });
        }
        Ok(Self(result))
    }

    /// The installed lookup builder 0x00e30060 keeps the last duplicate ID.
    pub fn row_mut(&mut self, id: u32) -> Option<&mut RandomRow> {
        self.0.iter_mut().rev().find(|row| row.id == id && id != 0xffff)
    }
}

impl RandomRow {
    /// One iteration of 0x00dcc120 (DDDA.exe 2017-12-13 Steam).
    /// None means draw again: an uncovered percentage or a forbidden repeat.
    /// The caller owns RNG and a retry budget; malformed data cannot hang here.
    pub fn draw(&mut self, random: u32) -> Option<i32> {
        let roll = random % 100 + 1;
        let mut cumulative = 0u32;
        for &(request, weight) in &self.choices {
            cumulative = cumulative.wrapping_add(weight);
            if roll <= cumulative {
                if !self.allow_repeat {
                    if request == self.previous { return None; }
                    self.previous = request;
                }
                return Some(request);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn weighted_boundaries_and_rejected_repeats_preserve_state() {
        let mut row = RandomRow { id: 7, choices: [(0, 0); 16], allow_repeat: false, previous: -1 };
        row.choices[0] = (20, 30);
        row.choices[1] = (21, 60);
        assert_eq!(row.draw(29), Some(20));
        assert_eq!(row.draw(100), None);
        assert_eq!(row.previous, 20);
        assert_eq!(row.draw(30), Some(21));
        assert_eq!(row.draw(99), None);
        assert_eq!(row.previous, 21);
        row.allow_repeat = true;
        assert_eq!(row.draw(0), Some(20));
        assert_eq!(row.draw(0), Some(20));
        assert_eq!(row.previous, 21);
    }
    #[test]
    fn file_rows_and_duplicate_lookup_match_loader() {
        let mut bytes = vec![0; 16 + 2 * 0x8c];
        bytes[..4].copy_from_slice(b"DNRS");
        bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
        for index in 0..2 {
            let at = 16 + index * 0x8c;
            bytes[at..at+4].copy_from_slice(&7u32.to_le_bytes());
            bytes[at+4..at+8].copy_from_slice(&(20+index as u32).to_le_bytes());
            bytes[at+8..at+12].copy_from_slice(&100u32.to_le_bytes());
            bytes[at+0x88..at+0x8c].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        let mut parsed = RandomRequests::parse(&bytes).unwrap();
        assert_eq!(parsed.row_mut(7).unwrap().draw(99), Some(21));
        assert!(parsed.row_mut(8).is_none());
        bytes.pop();
        assert!(RandomRequests::parse(&bytes).is_err());
    }
}
