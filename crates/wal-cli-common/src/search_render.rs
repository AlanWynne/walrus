//! The search report data model and its Markdown / plain renderers
//! (CLI-DESIGN 5.8).
//!
//! The data model is deliberately VERSION-AGNOSTIC. A [`SearchReport`] carries
//! the archive display string and a list of [`FileGroup`]s; every per-match
//! fact -- including the version index and that version's change timestamp --
//! lives on the individual [`walrus_fmt::Match`], never as a single
//! report-level scalar. The all-versions renderer groups these matches by
//! version index without changing the match data model.

use std::collections::BTreeMap;

use walrus_fmt::Match;

/// The output format for a rendered report (CLI-DESIGN 5.8). Markdown is the
/// default; plain is the pipe-friendly form that omits table syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// A Markdown report (default): a header plus a per-file table.
    #[default]
    Markdown,
    /// Plain text with no Markdown table syntax, for piping.
    Plain,
}

/// One file's matches within a report, in match (byte-offset) order.
///
/// A group never stores "the version index" as a scalar: the index and its
/// timestamp are read off each [`Match`], so all-versions reports can hold
/// matches from several versions in one group untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileGroup {
    /// The tracked file's stored name/path.
    pub file_name: String,
    /// Every match in this file, in the order the matcher reported them.
    pub matches: Vec<Match>,
}

/// A whole search report: the archive display string and the per-file groups.
///
/// The report scope selects single-version or all-versions rendering, while
/// each match carries its own version index and timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchReport {
    /// How the archive is displayed in the header (its path/name).
    pub archive_display: String,
    /// The searched version scope.
    pub scope: SearchScope,
    /// The per-file match groups, in archive (manifest) order.
    pub groups: Vec<FileGroup>,
}

/// Whether a search covers one version or the file's complete history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchScope {
    /// A single resolved version.
    Version {
        /// Concrete index searched in each selected file.
        index: usize,
        /// Timestamp of the change that produced the version, if available.
        timestamp: Option<String>,
    },
    /// Every version from current through original.
    AllVersions,
}

impl SearchReport {
    /// Total number of matches across every file.
    pub fn match_count(&self) -> usize {
        self.groups.iter().map(|g| g.matches.len()).sum()
    }

    /// Number of files that contain at least one match.
    pub fn file_hit_count(&self) -> usize {
        self.groups.iter().filter(|g| !g.matches.is_empty()).count()
    }

    /// Render the report in the requested format.
    pub fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Markdown => self.render_markdown(),
            OutputFormat::Plain => self.render_plain(),
        }
    }

    /// Render a Markdown report with the searched version(s), match tables,
    /// and a trailing match/file-count summary.
    fn render_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Search report\n\n");
        out.push_str(&format!("- Archive: `{}`\n", self.archive_display));
        match &self.scope {
            SearchScope::AllVersions => out.push_str("- Versions: all\n\n"),
            SearchScope::Version { index, timestamp } => {
                out.push_str(&format!("- Version index: {index}\n"));
                out.push_str(&format!(
                    "- Version timestamp: {}\n\n",
                    timestamp.as_deref().unwrap_or("-")
                ));
            }
        }

        for group in &self.groups {
            if group.matches.is_empty() {
                continue;
            }
            out.push_str(&format!("## `{}`\n\n", group.file_name));
            if self.scope == SearchScope::AllVersions {
                let mut by_version = BTreeMap::<usize, Vec<&Match>>::new();
                for matched in &group.matches {
                    by_version
                        .entry(matched.version_index)
                        .or_default()
                        .push(matched);
                }
                for (index, matches) in by_version {
                    let timestamp = matches
                        .first()
                        .and_then(|matched| matched.version_ts.as_deref())
                        .unwrap_or("-");
                    out.push_str(&format!(
                        "### Version index: {index} (timestamp: {timestamp})\n\n"
                    ));
                    render_markdown_table(&mut out, matches.iter().copied());
                    out.push('\n');
                }
            } else {
                render_markdown_table(&mut out, group.matches.iter());
                out.push('\n');
            }
        }

        out.push_str(&format!(
            "**{} match(es) in {} file(s).**\n",
            self.match_count(),
            self.file_hit_count()
        ));
        out
    }

    /// Render a plain-text report with no Markdown table syntax, for piping:
    /// one located-match line per match, then a summary line.
    fn render_plain(&self) -> String {
        let mut out = match &self.scope {
            SearchScope::AllVersions => format!("archive {} versions all\n", self.archive_display),
            SearchScope::Version { index, timestamp } => format!(
                "archive {} version {index} ts {}\n",
                self.archive_display,
                timestamp.as_deref().unwrap_or("-")
            ),
        };
        for group in &self.groups {
            for m in &group.matches {
                if self.scope == SearchScope::AllVersions {
                    out.push_str(&format!(
                        "{}:{}:{}:{}:{}:{}\n",
                        group.file_name,
                        m.version_index,
                        m.line,
                        m.col,
                        m.byte_offset,
                        m.matched_text.replace('\n', " ")
                    ));
                } else {
                    out.push_str(&format!(
                        "{}:{}:{}:{}:{}\n",
                        group.file_name,
                        m.line,
                        m.col,
                        m.byte_offset,
                        m.matched_text.replace('\n', " ")
                    ));
                }
            }
        }
        out.push_str(&format!(
            "{} match(es) in {} file(s)\n",
            self.match_count(),
            self.file_hit_count()
        ));
        out
    }
}

fn render_markdown_table<'a>(out: &mut String, matches: impl IntoIterator<Item = &'a Match>) {
    out.push_str("| Line | Col | Byte offset | Match |\n");
    out.push_str("| ---- | --- | ----------- | ----- |\n");
    for matched in matches {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            matched.line,
            matched.col,
            matched.byte_offset,
            escape_cell(&matched.matched_text)
        ));
    }
}

/// Escape a matched snippet so it stays on one Markdown table cell: pipes are
/// escaped and newlines/carriage returns become visible placeholders so a
/// multi-line regex match never breaks the table row.
fn escape_cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_match(file: &str, index: usize, ts: Option<&str>, text: &str) -> Match {
        Match {
            file_name: file.to_string(),
            version_index: index,
            version_ts: ts.map(str::to_string),
            byte_offset: 6,
            line: 1,
            col: 7,
            matched_text: text.to_string(),
        }
    }

    fn report() -> SearchReport {
        SearchReport {
            archive_display: "fixture.wal".to_string(),
            scope: SearchScope::Version {
                index: 0,
                timestamp: Some("2026-10-11T16:18:44Z".to_string()),
            },
            groups: vec![FileGroup {
                file_name: "notes.txt".to_string(),
                matches: vec![one_match(
                    "notes.txt",
                    0,
                    Some("2026-10-11T16:18:44Z"),
                    "walrus",
                )],
            }],
        }
    }

    #[test]
    fn markdown_header_carries_archive_index_and_timestamp() {
        let md = report().render(OutputFormat::Markdown);
        assert!(md.contains("fixture.wal"), "archive name in header");
        assert!(md.contains("Version index: 0"), "index in header");
        assert!(
            md.contains("2026-10-11T16:18:44Z"),
            "change timestamp in header"
        );
    }

    #[test]
    fn markdown_per_file_table_has_line_col_offset_match_columns() {
        let md = report().render(OutputFormat::Markdown);
        assert!(md.contains("| Line | Col | Byte offset | Match |"));
        // The data row carries the match's line/col/offset/text.
        assert!(md.contains("| 1 | 7 | 6 | walrus |"));
    }

    #[test]
    fn markdown_ends_with_a_match_and_file_count_summary() {
        let md = report().render(OutputFormat::Markdown);
        assert!(md.contains("1 match(es) in 1 file(s)"), "{md}");
    }

    #[test]
    fn plain_format_omits_markdown_table_syntax() {
        let plain = report().render(OutputFormat::Plain);
        assert!(
            !plain.contains("| Line |"),
            "plain output must not carry the Markdown table header"
        );
        assert!(plain.contains("notes.txt:1:7:6:walrus"));
    }

    #[test]
    fn match_and_file_counts_span_every_group() {
        let r = SearchReport {
            archive_display: "a.wal".to_string(),
            scope: SearchScope::Version {
                index: 0,
                timestamp: None,
            },
            groups: vec![
                FileGroup {
                    file_name: "a.txt".to_string(),
                    matches: vec![
                        one_match("a.txt", 0, None, "x"),
                        one_match("a.txt", 0, None, "x"),
                    ],
                },
                FileGroup {
                    file_name: "b.txt".to_string(),
                    matches: vec![],
                },
            ],
        };
        assert_eq!(r.match_count(), 2);
        assert_eq!(r.file_hit_count(), 1);
    }

    #[test]
    fn all_versions_markdown_groups_hits_by_index_and_timestamp() {
        let r = SearchReport {
            archive_display: "a.wal".to_string(),
            scope: SearchScope::AllVersions,
            groups: vec![FileGroup {
                file_name: "a.txt".to_string(),
                matches: vec![
                    one_match("a.txt", 0, Some("2026-10-11T00:00:00Z"), "x"),
                    one_match("a.txt", 2, None, "x"),
                ],
            }],
        };
        let markdown = r.render(OutputFormat::Markdown);
        assert!(markdown.contains("- Versions: all"));
        assert!(markdown.contains("### Version index: 0 (timestamp: 2026-10-11T00:00:00Z)"));
        assert!(markdown.contains("### Version index: 2 (timestamp: -)"));
    }

    #[test]
    fn all_versions_plain_output_includes_version_index() {
        let mut r = report();
        r.scope = SearchScope::AllVersions;
        let plain = r.render(OutputFormat::Plain);
        assert!(plain.contains("archive fixture.wal versions all"));
        assert!(plain.contains("notes.txt:0:1:7:6:walrus"));
    }
}
