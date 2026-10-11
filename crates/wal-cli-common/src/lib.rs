//! Shared internal CLI logic for the walrus command-line tools.
//!
//! This crate holds the behaviour that `walx` and the forthcoming `walrus`
//! binary both need: segment-aware mask globbing, extract naming safety
//! (zip-slip / reserved-name / sanitize), the version selector grammar (the
//! fixed UTC `--at` parser included), the typed [`error::CliError`], and the
//! `extract`/`cat` execution engines. It is a `publish = false` internal crate,
//! not part of the public `walrus-fmt` surface.
//!
//! The `names` module adds CLI-layer helpers (glob-to-exact-name expansion and
//! change-log byte accounting) that the `walrus-fmt` library deliberately does
//! not expose.

pub mod cat;
pub mod error;
pub mod extract;
pub mod mask;
pub mod names;
pub mod safety;
pub mod selector;
