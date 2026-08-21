//! Sketchi local-first desktop editor library.

#![forbid(unsafe_code)]

pub mod app;
mod components;
pub mod connection;
pub mod editor;
mod gpu;
mod images;
pub mod input;
#[path = "lucide_icons.rs"]
mod lucide_icons;
mod preview;
mod selection;
pub(crate) mod settings;
pub mod storage;
pub mod supervisor;
#[allow(dead_code)]
mod theme;
pub mod tools;
mod ui;
mod update;
mod window_state;
