//! The `salvage` command: one of the tools in Salvage, Chapterists' open-source
//! toolbox of the tools we use to make our stories.
//!
//! Its subcommands check, measure and set up Unreal Engine work: `doctor`,
//! `soundings`, `trim`, `lldb`, `clangd` and `refit`. The binary is a thin layer over this crate.

pub mod clangd;
pub mod doctor;
pub mod engine;
pub mod fsutil;
pub mod ini;
pub mod jsonc;
pub mod lldb;
pub mod probe;
pub mod project;
pub mod refit;
pub mod soundings;
pub mod trim;
pub mod ui;
pub mod units;
pub mod version;

#[cfg(test)]
pub(crate) mod testing;
