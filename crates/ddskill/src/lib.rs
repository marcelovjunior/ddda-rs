//! Vocations, skills, augments and level growth for Dragon's Dogma: Dark Arisen.
//!
//! * [`skill_list`]: reader for `rSkillList` (`etc\skill\skill`, magic `skl\0`),
//!   the learning costs of core skills, weapon skills and augments.
//! * [`growth`]: level-up, experience curve, discipline (job points) and
//!   vocation rank rules, translated from DDDA.exe.
//! * [`vocation`]: vocation ids and the vocation-change rules.
//! * [`catalogue`]: every skill and augment, joining `rSkillList`, tables read
//!   at run time from the player's `DDDA.exe` (through `ddexe`) and the game
//!   text.
//!
//! Every translated rule names its source address in DDDA.exe 2017-12-13 Steam
//! (identity in `docs/METHODOLOGY.md`). Findings and open questions are in
//! `docs/VOCATIONS.md`. Tests say which values were compared with the running
//! game.

pub mod catalogue;
pub mod growth;
pub mod magic;
pub mod skill_list;
pub mod vocation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<ddexe::Error> for Error {
    fn from(e: ddexe::Error) -> Self {
        Error(e.0)
    }
}
