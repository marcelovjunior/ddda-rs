//! Readers for Dragon's Dogma: Dark Arisen item, shop, recipe and
//! enhancement tables, and the shop price and stock rules traced from the
//! exe (see `docs/SHOPS.md`). Headless; no game data is embedded.

mod bin;
pub mod itl;
pub mod lvparam;
pub mod lvup;
pub mod npc;
pub mod rules;
pub mod shop;
pub mod tables;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl Error {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}
