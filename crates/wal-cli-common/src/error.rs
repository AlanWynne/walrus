//! The typed CLI error carrying the process return code it maps to
//! (CLI-DESIGN section 6).
//!
//! Only the codes the arg layer raises appear here; richer per-file outcomes
//! are decided in the command modules.

use std::fmt;

/// A usage/safety error carrying the process return code it maps to
/// (CLI-DESIGN section 6). Only the codes raised at the arg layer appear here;
/// richer per-file outcomes are decided in the command modules.
#[derive(Debug, PartialEq, Eq)]
pub struct CliError {
    /// Human-readable reason, printed to stderr by `main`.
    pub message: String,
    /// The process return code (2 usage/safety for everything parsed here).
    pub code: u8,
}

impl CliError {
    /// Build a usage/safety error (RC 2).
    pub fn usage(message: impl Into<String>) -> Self {
        CliError {
            message: message.into(),
            code: 2,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}
