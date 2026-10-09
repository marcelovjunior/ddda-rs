//! `rNpcLedgerList` (`etc\item\NpcList`): the NPC ledger. Only what the
//! shops need is interpreted here: the number (which names the shop file
//! `etc\shop\n%03dShopList`), the shop type and the internal name.
//!
//! Loader: `rNpcLedgerList::vf10` at `0x00ccac90` (magic `nnl\0`, version
//! 8). Per record: eight u16, six u8, two NUL-terminated strings (read into
//! 32-byte buffers). Names and memory offsets from `cParam::vf03`
//! (`0x00cca670`); the order of the u16/u8 reads follows memory order
//! +0x4..+0x19.

use crate::bin::Cursor;
use crate::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NpcLedgerEntry {
    pub no: u16,
    pub stage_no: u16,
    pub init_friend_point: u16,
    pub model_type: u16,
    pub index: u16,
    pub stray_id: u16,
    pub shop_type: u16,
    pub se_type: u16,
    pub flag: u8,
    pub child: u8,
    pub tp: u8,
    pub tp_type: u8,
    pub like_item: u8,
    pub civilian: u8,
    /// `mName`: internal romanised name (e.g. `Adaro`), not the displayed
    /// name.
    pub name: String,
    pub name_j: String,
}

pub fn parse_npc_ledger(data: &[u8]) -> Result<Vec<NpcLedgerEntry>, Error> {
    let mut c = Cursor::new(data);
    let n = c.header(b"nnl\0", 8)?;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let no = c.u16()?;
        let stage_no = c.u16()?;
        let init_friend_point = c.u16()?;
        let model_type = c.u16()?;
        let index = c.u16()?;
        let stray_id = c.u16()?;
        let shop_type = c.u16()?;
        let se_type = c.u16()?;
        let flag = c.u8()?;
        let child = c.u8()?;
        let tp = c.u8()?;
        let tp_type = c.u8()?;
        let like_item = c.u8()?;
        let civilian = c.u8()?;
        let name = String::from_utf8_lossy(c.cstr()?).into_owned();
        let name_j = String::from_utf8_lossy(c.cstr()?).into_owned();
        out.push(NpcLedgerEntry {
            no,
            stage_no,
            init_friend_point,
            model_type,
            index,
            stray_id,
            shop_type,
            se_type,
            flag,
            child,
            tp,
            tp_type,
            like_item,
            civilian,
            name,
            name_j,
        });
    }
    c.finish()?;
    Ok(out)
}
