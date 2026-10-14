//! FR-K7 bloat classifier: the pure logic behind the internal-pane row band.
//!
//! A file's bloat ratio is `R = log_bytes / size_bytes` (change-log size over
//! current size). The classifier maps `R` to a [`BloatBand`] against
//! configurable [`BloatThresholds`], with an index-count FLOOR that keeps a
//! file GREEN until it has accumulated enough versions to be worth warning
//! about. The band is advisory only: nothing here mutates an archive.
//!
//! This module is deliberately egui-free in its core: [`classify`], the band
//! enum, and the thresholds are plain data and are unit-tested without a UI.
//! Only the small [`band_color`] helper names an `egui` type, so the row can
//! paint the band; its exact shades are a MANUAL visual check.

use eframe::egui;

// === Thresholds =====================================================

/// Default ratio at or above which a file leaves GREEN for AMBER.
const DEFAULT_AMBER_AT: f64 = 2.0;
/// Default ratio at or above which a file is RED.
const DEFAULT_RED_AT: f64 = 5.0;
/// Default index-count floor: below this many versions a file stays GREEN
/// regardless of its ratio (too little history to warrant a warning).
const DEFAULT_FLOOR_INDEX_COUNT: usize = 50;

/// User-configurable thresholds for the bloat bands.
///
/// `amber_at` and `red_at` are ratio cut-points (`R = log_bytes / size_bytes`);
/// `floor_index_count` is the minimum version count before the ratio is allowed
/// to raise a warning. The [`Default`] carries the FR-K7 starting defaults
/// (`2.0` / `5.0` / `50`); it is the single overridable source of truth so the
/// row never hard-codes a literal.
#[derive(Debug, Clone, Copy)]
pub struct BloatThresholds {
    /// Ratio at or above which the band becomes AMBER.
    pub amber_at: f64,
    /// Ratio at or above which the band becomes RED.
    pub red_at: f64,
    /// Minimum index (version) count before the ratio can raise the band above
    /// GREEN.
    pub floor_index_count: usize,
}

impl Default for BloatThresholds {
    fn default() -> Self {
        Self {
            amber_at: DEFAULT_AMBER_AT,
            red_at: DEFAULT_RED_AT,
            floor_index_count: DEFAULT_FLOOR_INDEX_COUNT,
        }
    }
}

// === Band ===========================================================

/// The advisory bloat band a file falls into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BloatBand {
    /// Healthy: ratio below `amber_at`, or below the index-count floor.
    Green,
    /// Watch: `amber_at <= R < red_at`.
    Amber,
    /// Bloated: `R >= red_at`.
    Red,
}

// === Classifier =====================================================

/// Classify a file's bloat band from its raw figures and the thresholds.
///
/// Logic order: a file under the index-count floor is always GREEN (too little
/// history to warn about); a file with `size_bytes == 0` is GREEN because the
/// ratio is undefined and an advisory indicator must never claim bloat it
/// cannot substantiate; otherwise the ratio `R = log_bytes / size_bytes` is
/// banded by `amber_at` / `red_at`.
pub fn classify(
    log_bytes: u64,
    size_bytes: u64,
    index_count: usize,
    thresholds: &BloatThresholds,
) -> BloatBand {
    // Too little history to warn about: stay GREEN regardless of ratio.
    if index_count < thresholds.floor_index_count {
        return BloatBand::Green;
    }
    // Undefined ratio: an advisory indicator must never claim bloat it cannot
    // substantiate, so a zero current size reads GREEN (also avoids NaN/inf).
    if size_bytes == 0 {
        return BloatBand::Green;
    }
    let ratio = log_bytes as f64 / size_bytes as f64;
    if ratio < thresholds.amber_at {
        BloatBand::Green
    } else if ratio < thresholds.red_at {
        BloatBand::Amber
    } else {
        BloatBand::Red
    }
}

/// The colour used to paint a band on a row.
///
/// The exact shades are a MANUAL visual check (egui/theme colour resolution is
/// not headless-assertable), so there is no behavioural test for this helper.
pub fn band_color(band: BloatBand) -> egui::Color32 {
    match band {
        BloatBand::Green => egui::Color32::from_rgb(0x2E, 0x7D, 0x32),
        BloatBand::Amber => egui::Color32::from_rgb(0xF9, 0xA8, 0x25),
        BloatBand::Red => egui::Color32::from_rgb(0xC6, 0x28, 0x28),
    }
}

/// Build the advisory tooltip text: index count, log bytes, and the ratio.
///
/// The ratio is formatted to two decimals; when `size_bytes == 0` the ratio is
/// undefined and shown as `n/a`. ASCII only, panic-free.
pub fn tooltip_text(
    index_count: usize,
    log_bytes: u64,
    size_bytes: u64,
    band: BloatBand,
) -> String {
    let ratio = if size_bytes == 0 {
        "n/a".to_string()
    } else {
        format!("{:.2}", log_bytes as f64 / size_bytes as f64)
    };
    format!(
        "bloat: {} -- {index_count} versions, {log_bytes} log bytes, ratio {ratio}",
        band_name(band),
    )
}

/// The advisory tag text drawn before a row's label, e.g. `[bloat: GREEN]`.
///
/// This is the stable, non-pixel handle a headless harness targets to assert
/// which band a row was given (the colour itself is a MANUAL visual check).
pub fn band_label(band: BloatBand) -> String {
    format!("[bloat: {}]", band_name(band))
}

/// The ASCII band name used in labels and tooltips.
fn band_name(band: BloatBand) -> &'static str {
    match band {
        BloatBand::Green => "GREEN",
        BloatBand::Amber => "AMBER",
        BloatBand::Red => "RED",
    }
}

// === Tests ==========================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn def() -> BloatThresholds {
        BloatThresholds::default()
    }

    #[test]
    fn thresholds_default_is_2_5_50() {
        let d = BloatThresholds::default();
        assert_eq!(d.amber_at, 2.0);
        assert_eq!(d.red_at, 5.0);
        assert_eq!(d.floor_index_count, 50);
    }

    #[test]
    fn ratio_below_amber_threshold_is_green() {
        // R = 1.0, index count over the floor.
        assert_eq!(classify(100, 100, 50, &def()), BloatBand::Green);
    }

    #[test]
    fn ratio_at_amber_threshold_is_amber() {
        // R = 2.0 exactly: the amber band is inclusive at its lower bound.
        assert_eq!(classify(200, 100, 50, &def()), BloatBand::Amber);
    }

    #[test]
    fn ratio_just_below_red_threshold_is_amber() {
        // R = 4.99 < 5.0.
        assert_eq!(classify(499, 100, 50, &def()), BloatBand::Amber);
    }

    #[test]
    fn ratio_at_or_above_red_threshold_is_red() {
        // R = 5.0 exactly, and R = 6.0.
        assert_eq!(classify(500, 100, 50, &def()), BloatBand::Red);
        assert_eq!(classify(600, 100, 50, &def()), BloatBand::Red);
    }

    #[test]
    fn under_fifty_versions_stays_green_despite_high_ratio() {
        // R = 100.0 but only 49 versions: the floor forces GREEN (AC1).
        assert_eq!(classify(1000, 10, 49, &def()), BloatBand::Green);
        // At exactly the floor the ratio takes over and this is RED.
        assert_eq!(classify(1000, 10, 50, &def()), BloatBand::Red);
    }

    #[test]
    fn at_the_floor_each_band_is_reachable() {
        // index_count == floor: ratio decides.
        assert_eq!(classify(100, 100, 50, &def()), BloatBand::Green);
        assert_eq!(classify(300, 100, 50, &def()), BloatBand::Amber);
        assert_eq!(classify(600, 100, 50, &def()), BloatBand::Red);
    }

    #[test]
    fn zero_size_bytes_is_green() {
        // Degenerate denominator (D2): undefined ratio -> GREEN, even over the
        // floor, so the indicator never claims bloat it cannot substantiate.
        assert_eq!(classify(1000, 0, 100, &def()), BloatBand::Green);
    }

    #[test]
    fn bounded_file_is_classified_by_its_actual_ratio() {
        // AC2: a bounded (generations=N) file whose log was re-based has a
        // small real ratio; it must read GREEN by its real numbers, not be
        // falsely inflated. R = 1.2 over the floor -> GREEN.
        assert_eq!(classify(120, 100, 60, &def()), BloatBand::Green);
    }

    #[test]
    fn tooltip_text_includes_index_count_log_bytes_and_ratio() {
        let text = tooltip_text(60, 300, 100, BloatBand::Amber);
        assert!(text.contains("60"), "shows the index count: {text}");
        assert!(text.contains("300"), "shows the log bytes: {text}");
        assert!(
            text.contains("3.00"),
            "shows the ratio to two decimals: {text}"
        );
    }

    #[test]
    fn tooltip_text_shows_na_ratio_when_size_is_zero() {
        let text = tooltip_text(60, 300, 0, BloatBand::Green);
        assert!(text.contains("n/a"), "undefined ratio shows n/a: {text}");
    }
}
