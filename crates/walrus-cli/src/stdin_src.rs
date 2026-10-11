//! The `-` + `--name` stdin ingestion grammar shared by `add` and `update`
//! (CLI-DESIGN 5.4e, FR-W11 AC9).
//!
//! A single `-` operand reads content from stdin and REQUIRES `--name`. The
//! rules enforced here (all RC 2): `-` requires `--name`; `--name` requires a
//! `-` source; `-` appears at most once and is NOT mixed with file arguments;
//! `-R` is not combined with `-`. The `--name` value is validated through the
//! shared naming safety (`wal_cli_common::safety`): an escaping or
//! unrepresentable name is RC 2 (AC9f). The buffered bytes then funnel through
//! the same `Archive` path as a file argument.

use std::io::Read;

use wal_cli_common::error::CliError;
use wal_cli_common::safety::{self, NameRefusal};

/// A validated stdin source: the buffered bytes and the stored name.
pub struct StdinSource {
    /// The stored archive name (`--name`, verbatim, `/`-separated).
    pub name: String,
    /// The bytes read from stdin to EOF.
    pub bytes: Vec<u8>,
}

/// Validate the `-` / `--name` / `-R` combination and, when a `-` source is
/// present, buffer stdin and return the [`StdinSource`]. Returns `Ok(None)`
/// when there is no stdin source (the command uses file arguments).
///
/// `dash_count` is how many `-` operands appeared; `name` is the `--name`
/// value if any; `file_count` is the number of non-`-` positional file
/// arguments; `recurse` is whether `-R` was given.
///
/// # Errors
/// Returns [`CliError`] (RC 2) for any violated rule (AC9b/c/f).
pub fn resolve(
    dash_count: usize,
    name: Option<&str>,
    file_count: usize,
    recurse: bool,
) -> Result<Option<StdinSource>, CliError> {
    if dash_count == 0 {
        // No stdin source: `--name` is meaningful only with `-` (AC9b).
        if name.is_some() {
            return Err(CliError::usage(
                "--name is only valid with a '-' stdin source",
            ));
        }
        return Ok(None);
    }

    if dash_count > 1 {
        return Err(CliError::usage("'-' may appear at most once"));
    }
    if file_count > 0 {
        return Err(CliError::usage(
            "'-' (stdin) may not be mixed with file arguments",
        ));
    }
    if recurse {
        return Err(CliError::usage("-R / --recurse is meaningless with '-'"));
    }
    let name = name.ok_or_else(|| CliError::usage("'-' (stdin) requires --name <stored-name>"))?;
    validate_name(name)?;

    let bytes = read_stdin()?;
    Ok(Some(StdinSource {
        name: name.to_string(),
        bytes,
    }))
}

/// Validate a `--name` through the shared extract naming safety: a path-escape
/// or unrepresentable name is RC 2 (AC9f). The name is stored verbatim.
fn validate_name(name: &str) -> Result<(), CliError> {
    match safety::classify(name) {
        Ok(()) => Ok(()),
        Err(NameRefusal::ZipSlip) => Err(CliError::usage(format!(
            "--name '{name}' escapes the archive (absolute or '..' path)"
        ))),
        Err(NameRefusal::Unrepresentable(_)) => Err(CliError::usage(format!(
            "--name '{name}' is not a representable stored name"
        ))),
    }
}

/// Read all of stdin into a buffer.
fn read_stdin() -> Result<Vec<u8>, CliError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|e| CliError::usage(format!("reading stdin: {e}")))?;
    Ok(bytes)
}
