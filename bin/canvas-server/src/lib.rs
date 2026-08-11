//! Collaboration server foundation.
//!
//! Room state and transport code will be added on top of the shared
//! `canvas-core` document engine.

#![forbid(unsafe_code)]

pub mod actor;
pub mod auth;
pub mod config;
pub mod error;
pub mod room;
pub mod store;
pub mod tls;
pub mod websocket;
