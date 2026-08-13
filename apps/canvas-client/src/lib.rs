//! Sketchi local-first desktop editor library.

#![forbid(unsafe_code)]

pub mod app;
mod components;
pub mod connection;
pub mod editor;
mod gpu;
mod images;
pub mod input;
mod preview;
mod remix_icons;
mod selection;
pub(crate) mod settings;
pub mod storage;
pub mod supervisor;
#[allow(dead_code)]
mod theme;
pub mod tools;
mod ui;
mod window_state;
