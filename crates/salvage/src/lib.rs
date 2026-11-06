//! Salvage: get more out of the hardware you already have.
//!
//! Tools for running and developing Unreal Engine projects on lower-powered
//! machines and on macOS. The `salvage` binary is a thin layer over this crate.

pub mod doctor;
pub mod engine;
pub mod ini;
pub mod jsonc;
pub mod probe;
pub mod project;
pub mod soundings;
pub mod ui;
pub mod units;
pub mod version;

#[cfg(test)]
pub(crate) mod testing;
