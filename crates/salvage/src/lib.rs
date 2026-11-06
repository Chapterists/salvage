//! Salvage: Chapterists' open-source toolbox, the tools we use to make our stories.
//!
//! The first tools are for Unreal Engine: running games and the editor on
//! lower-powered machines, and developing on macOS. The `salvage` binary is a
//! thin layer over this crate.

pub mod doctor;
pub mod engine;
pub mod ini;
pub mod jsonc;
pub mod lldb;
pub mod probe;
pub mod project;
pub mod soundings;
pub mod ui;
pub mod units;
pub mod version;

#[cfg(test)]
pub(crate) mod testing;
