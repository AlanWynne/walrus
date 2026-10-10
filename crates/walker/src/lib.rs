//! walker library surface.
//!
//! The `walker` binary is a thin shell over this library crate. The modules are
//! exposed here (rather than kept private to `main.rs`) so integration tests --
//! notably the headless `egui_kittest` harness in `tests/` -- can drive the
//! real `player` state machine and the `app` shell.

pub mod app;
pub mod chrome;
pub mod export_ui;
pub mod navigator;
pub mod player;
pub mod source;
pub mod theme;
pub mod transport;
pub mod view;
