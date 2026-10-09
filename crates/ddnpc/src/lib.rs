//! Headless readers for the DDDA NPC ledger and dialogue resources.
//! See docs/NPCS.md for evidence and unresolved selection semantics.
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| Error("size overflow".into()))?;
        let value = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| Error(format!("truncated at {:#x}, need {n} bytes", self.pos)))?;
        self.pos = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    fn array<T: Default + Copy, const N: usize>(
        &mut self,
        read: fn(&mut Self) -> Result<T>,
    ) -> Result<[T; N]> {
        let mut values = [T::default(); N];
        for value in &mut values {
            *value = read(self)?;
        }
        Ok(values)
    }
    fn string(&mut self) -> Result<&'a [u8]> {
        let n = self.bytes[self.pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error("unterminated ledger name".into()))?;
        let bytes = self.take(n)?;
        self.take(1)?;
        Ok(bytes)
    }
    fn header(&mut self, magic: &[u8; 4], version: u32, minimum: usize) -> Result<usize> {
        if self.take(4)? != magic || self.u32()? != version {
            return Err(Error("unsupported magic/version".into()));
        }
        let count = self.u32()? as usize;
        if count > (self.bytes.len() - self.pos) / minimum {
            return Err(Error("count exceeds available records".into()));
        }
        Ok(count)
    }
    fn finish(self) -> Result<()> {
        if self.pos != self.bytes.len() {
            Err(Error(format!("{} trailing bytes", self.bytes.len() - self.pos)))
        } else {
            Ok(())
        }
    }
}

/// Names retain their serialized bytes; their encoding is not assumed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Npc<'a> {
    pub id: i16,
    pub stage: i16,
    pub initial_friendship: i16,
    pub model_type: u16,
    pub model_index: u16,
    pub stray_id: i16,
    pub shop_type: i16,
    pub voice_type: i16,
    pub flag: i8,
    pub child: i8,
    pub tp: i8,
    pub tp_type: i8,
    pub like_item: i8,
    pub civilian: i8,
    pub name: &'a [u8],
    pub name_j: &'a [u8],
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ledger<'a> {
    pub entries: Vec<Npc<'a>>,
}
impl<'a> Ledger<'a> {
    /// Translated from 0x00ccac90; field names from 0x00cca670
    /// (DDDA.exe 2017-12-13 Steam). Variable NUL strings, not 100-byte rows.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut r = Reader { bytes, pos: 0 };
        let count = r.header(b"nnl\0", 8, 24)?;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(Npc {
                id: r.i16()?,
                stage: r.i16()?,
                initial_friendship: r.i16()?,
                model_type: r.u16()?,
                model_index: r.u16()?,
                stray_id: r.i16()?,
                shop_type: r.i16()?,
                voice_type: r.i16()?,
                flag: r.i8()?,
                child: r.i8()?,
                tp: r.i8()?,
                tp_type: r.i8()?,
                like_item: r.i8()?,
                civilian: r.i8()?,
                name: r.string()?,
                name_j: r.string()?,
            });
        }
        r.finish()?;
        Ok(Self { entries })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSerial {
    pub numbers: Vec<u16>,
}
impl MessageSerial {
    /// Translated from 0x00cc8ce0 (DDDA.exe 2017-12-13 Steam).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader { bytes, pos: 0 };
        let count = r.header(b"msl\0", 0x10, 2)?;
        let numbers = (0..count).map(|_| r.u16()).collect::<Result<_>>()?;
        r.finish()?;
        Ok(Self { numbers })
    }
}

/// One serialized 250-byte `rMsgSet::cParam`. Signed sentinel values are retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub number: i32,
    pub npc_id: i16,
    pub choices: [i32; 6],
    pub choice_jumps: [i16; 6],
    pub messages: [i32; 15],
    pub motions: [i16; 15],
    pub faces: [i8; 15],
    pub global_flags: [i16; 15],
    pub quest_numbers: [i16; 15],
    pub quest_flags: [i16; 15],
    pub season: i8,
    pub season_sequence: i8,
    pub season_sequence_type: i8,
    pub quest_number: i16,
    pub quest_flag: i16,
    pub friendship: i16,
    pub friendship_max: i16,
    pub global_flag: i16,
}
impl Conversation {
    /// Active prefix ends at the first -1; later slots are preserved by the reader.
    /// Translated from 0x00cc95f0 (DDDA.exe 2017-12-13 Steam).
    pub fn active_choices(&self) -> &[i32] {
        &self.choices[..self.choices.iter().position(|&v| v == -1).unwrap_or(6)]
    }
    /// Serialized message prefix (up to 15). Does not apply runtime conditions.
    pub fn active_messages(&self) -> &[i32] {
        &self.messages[..self.messages.iter().position(|&v| v == -1).unwrap_or(15)]
    }
    /// The loader writes the third short array over `mQuestNo_msg` at +0x130.
    /// This exposes that overwrite without destroying either serialized array.
    /// Translated from 0x00cc95f0 (DDDA.exe 2017-12-13 Steam).
    pub fn loaded_quest_numbers(&self) -> &[i16; 15] {
        &self.quest_flags
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageSet {
    pub conversations: Vec<Conversation>,
}
impl MessageSet {
    /// First conversation with this `mNo`, not the array index.
    /// Translated from 0x00cc9ca0 (DDDA.exe 2017-12-13 Steam).
    pub fn by_number(&self, number: i32) -> Option<&Conversation> {
        self.conversations.iter().find(|c| c.number == number)
    }
    /// A line index in the paired GMD for a freshly loaded MSS.
    /// This does not select a conversation by season, affinity or quest state.
    /// Global flags are delegated to the caller's headless world state.
    /// The loader leaves runtime quest flags at -1, so per-line quest checks
    /// are skipped. Runtime edits to those flags are outside this API.
    /// Translated from 0x00cc9ca0, 0x00cc95f0 and 0x00cc8ec0
    /// (DDDA.exe 2017-12-13 Steam).
    pub fn loaded_message_index(
        &self,
        number: i32,
        line: usize,
        mut global_flag_is_set: impl FnMut(i16) -> bool,
    ) -> Option<i32> {
        let row = self.by_number(number)?;
        let &index = row.messages.get(line)?;
        if index == -1 {
            return None;
        }
        let flag = row.global_flags[line];
        if flag != -1 && !global_flag_is_set(flag) {
            return None;
        }
        Some(index)
    }
    /// Translated from 0x00cc95f0; names from 0x00cc8f60
    /// (DDDA.exe 2017-12-13 Steam). Retains the arrays lost by the engine loader.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader { bytes, pos: 0 };
        let count = r.header(b"mss\0", 0x20, 250)?;
        let mut conversations = Vec::with_capacity(count);
        for _ in 0..count {
            conversations.push(Conversation {
                number: r.i32()?,
                npc_id: r.i16()?,
                choices: r.array(Reader::i32)?,
                choice_jumps: r.array(Reader::i16)?,
                messages: r.array(Reader::i32)?,
                motions: r.array(Reader::i16)?,
                faces: r.array(|r| Ok(r.u8()? as i8))?,
                global_flags: r.array(Reader::i16)?,
                quest_numbers: r.array(Reader::i16)?,
                quest_flags: r.array(Reader::i16)?,
                season: r.u8()? as i8,
                season_sequence: r.u8()? as i8,
                season_sequence_type: r.u8()? as i8,
                quest_number: r.i16()?,
                quest_flag: r.i16()?,
                friendship: r.i16()?,
                friendship_max: r.i16()?,
                global_flag: r.i16()?,
            });
        }
        r.finish()?;
        Ok(Self { conversations })
    }
}

/// Select the 24-hour controller path for one NPC placement.
/// This is the intel-backed placement path, not uNpc's separate fallback.
/// No resource existence check or quest override is performed here.
/// Translated from 0x00bbdaa0 / 0x00bbdc80 (DDDA.exe 2017-12-13 Steam).
pub fn schedule_path(stage: i32, scenario: i32, npc_id: i32, use_24_schedule: u8) -> Option<String> {
    if !(0..900).contains(&npc_id) || use_24_schedule != 1 {
        return None;
    }
    let suffix = if stage == 200 && scenario < 3600 {
        "_00"
    } else if (stage == 200 || stage == 240) && scenario >= 7800 {
        "_01"
    } else {
        ""
    };
    Some(format!(
        "AI\\FSM\\Npc\\st{stage:03}\\st{stage:03}_n{npc_id:03}{suffix}"
    ))
}

/// The schedule controller's ten-minute bucket. Returns None for an invalid
/// clock rather than silently wrapping; the original receives a valid game clock.
/// Translated from 0x0059d790 (DDDA.exe 2017-12-13 Steam).
pub fn schedule_time(hour: u32, minute: u32) -> Option<u32> {
    (hour < 24 && minute < 60).then(|| hour * 6 + minute / 10)
}
