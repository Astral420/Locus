//! Decide whether OCR "slides" are a real presentation or just a screen
//! recording, so screen text does not compete with the speech in the summary.
//!
//! The frame detector saves a "slide" whenever a frame differs from the last
//! one, so desktop recordings produce many text-heavy, chrome-like rows.
//! Excluding a real presentation's slides only loses some detail while the
//! speech remains the primary source; including screen chrome corrupted the
//! summary. The heuristic therefore fails toward screen capture.

use super::summarization::SlideRow;

/// A slide averaging this many characters is dense enough to be a screen.
const SCREEN_AVG_CHARS: usize = 1_200;
/// This density is a screen when enough of its lines look like chrome.
const SCREEN_AVG_CHARS_WITH_CHROME: usize = 700;
/// Chrome-like share of lines (in percent) that turns the second rule on.
const SCREEN_CHROME_PERCENT: usize = 25;
/// Frequent frame changes on top of moderate text mean a screen.
const SCREEN_CHANGES_PER_MINUTE: f64 = 8.0;
/// The rate rule only applies above this average density.
const SCREEN_RAPID_AVG_CHARS: usize = 400;

/// Substrings that mark a line as application chrome rather than slide prose.
const CHROME_SUBSTRINGS: &[&str] = &["://", "www."];
/// Prefixes that mark shell prompts, paths and quoted lines.
const CHROME_PREFIXES: &[&str] = &["/", "~/", "$ ", "% ", "> "];
/// File extensions that mark a source or web file name.
const CHROME_EXTENSIONS: &[&str] = &[
    ".rs", ".ts", ".tsx", ".js", ".py", ".json", ".html", ".css", ".md", ".toml",
];

/// Counts taken from the raw OCR rows and the meeting timeline for one
/// summary. Raw rows are used on purpose: density is the signal.
#[derive(Debug, Clone, PartialEq)]
pub struct SlideSourceStats {
    pub unique_slides: usize,
    pub total_chars: usize,
    pub max_chars: usize,
    pub total_lines: usize,
    pub chrome_like_lines: usize,
    pub occurrences: usize,
    /// Zero means the duration is unknown, which disables the rate rule.
    pub duration_seconds: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlideSourceKind {
    Presentation,
    ScreenCapture,
}

impl SlideSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Presentation => "presentation",
            Self::ScreenCapture => "screen_capture",
        }
    }
}

/// The classification plus a short sentence with the numbers, used only in logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlideSourceDecision {
    pub kind: SlideSourceKind,
    pub reason: String,
}

/// Builds the classification input from the raw slide rows. `occurrences` is the
/// number of `slide_occurrences` rows and `duration_seconds` the meeting length.
pub(crate) fn build_stats(
    slides: &[SlideRow],
    occurrences: usize,
    duration_seconds: f32,
) -> SlideSourceStats {
    let mut stats = SlideSourceStats {
        unique_slides: slides.len(),
        total_chars: 0,
        max_chars: 0,
        total_lines: 0,
        chrome_like_lines: 0,
        occurrences,
        duration_seconds: duration_seconds.max(0.0),
    };
    for slide in slides {
        let chars = slide.ocr_text.chars().count();
        stats.total_chars += chars;
        stats.max_chars = stats.max_chars.max(chars);
        for line in slide.ocr_text.lines() {
            stats.total_lines += 1;
            if is_chrome_like(line) {
                stats.chrome_like_lines += 1;
            }
        }
    }
    stats
}

pub fn classify_slide_source(stats: &SlideSourceStats) -> SlideSourceDecision {
    if stats.unique_slides == 0 {
        return SlideSourceDecision {
            kind: SlideSourceKind::Presentation,
            reason: "no slides".into(),
        };
    }

    let avg_chars = stats.total_chars / stats.unique_slides;
    let chrome_percent = stats
        .chrome_like_lines
        .saturating_mul(100)
        .checked_div(stats.total_lines)
        .unwrap_or(0);
    let changes_per_minute = (stats.duration_seconds > 0.0)
        .then(|| stats.occurrences as f64 / (f64::from(stats.duration_seconds) / 60.0));

    if avg_chars >= SCREEN_AVG_CHARS {
        return SlideSourceDecision {
            kind: SlideSourceKind::ScreenCapture,
            reason: format!(
                "{avg_chars} average characters per slide across {} slides",
                stats.unique_slides
            ),
        };
    }
    if avg_chars >= SCREEN_AVG_CHARS_WITH_CHROME && chrome_percent >= SCREEN_CHROME_PERCENT {
        return SlideSourceDecision {
            kind: SlideSourceKind::ScreenCapture,
            reason: format!(
                "{avg_chars} average characters per slide with {chrome_percent}% chrome-like lines"
            ),
        };
    }
    if let Some(rate) = changes_per_minute {
        if rate >= SCREEN_CHANGES_PER_MINUTE && avg_chars >= SCREEN_RAPID_AVG_CHARS {
            return SlideSourceDecision {
                kind: SlideSourceKind::ScreenCapture,
                reason: format!(
                    "{rate:.1} slide changes per minute with {avg_chars} average characters per slide"
                ),
            };
        }
    }

    SlideSourceDecision {
        kind: SlideSourceKind::Presentation,
        reason: format!(
            "{avg_chars} average characters per slide across {} slides",
            stats.unique_slides
        ),
    }
}

/// Language-agnostic signals of application chrome: URLs, paths, shell prompts
/// and source or web file names.
pub fn is_chrome_like(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    if CHROME_SUBSTRINGS.iter().any(|marker| line.contains(marker)) {
        return true;
    }
    if CHROME_PREFIXES
        .iter()
        .any(|prefix| line.starts_with(prefix))
    {
        return true;
    }
    is_drive_path(line) || has_code_extension(line)
}

fn is_drive_path(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn has_code_extension(line: &str) -> bool {
    line.split_whitespace().any(|token| {
        let token = token.trim_matches(|character: char| {
            matches!(
                character,
                ',' | ';' | ':' | ')' | '(' | ']' | '[' | '"' | '\'' | '`'
            )
        });
        let token = token.to_ascii_lowercase();
        CHROME_EXTENSIONS
            .iter()
            .any(|extension| token.len() > extension.len() && token.ends_with(extension))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(
        unique_slides: usize,
        total_chars: usize,
        total_lines: usize,
        chrome_like_lines: usize,
        occurrences: usize,
        duration_seconds: f32,
    ) -> SlideSourceDecision {
        classify_slide_source(&SlideSourceStats {
            unique_slides,
            total_chars,
            max_chars: 0,
            total_lines,
            chrome_like_lines,
            occurrences,
            duration_seconds,
        })
    }

    fn slide(id: &str, timestamp_seconds: f32, ocr_text: &str) -> SlideRow {
        SlideRow {
            id: id.to_owned(),
            timestamp_seconds,
            ocr_text: ocr_text.to_owned(),
        }
    }

    #[test]
    fn the_measured_screen_recording_is_a_screen_capture() {
        // 5 slides, 12,181 characters total, 3,329 in the largest.
        let stats = SlideSourceStats {
            unique_slides: 5,
            total_chars: 12_181,
            max_chars: 3_329,
            total_lines: 0,
            chrome_like_lines: 0,
            occurrences: 30,
            duration_seconds: 1_200.0,
        };
        let decision = classify_slide_source(&stats);
        assert_eq!(decision.kind, SlideSourceKind::ScreenCapture);
        assert!(decision.reason.contains("2436"), "{}", decision.reason);
    }

    #[test]
    fn a_typical_deck_is_a_presentation() {
        // 20 slides of about 180 characters, few chrome-like lines.
        let decision = classify(20, 20 * 180, 20 * 5, 5, 20, 20.0 * 60.0);
        assert_eq!(decision.kind, SlideSourceKind::Presentation);
    }

    #[test]
    fn a_text_heavy_lecture_deck_is_a_presentation() {
        let decision = classify(10, 10 * 600, 10 * 8, 0, 10, 30.0 * 60.0);
        assert_eq!(decision.kind, SlideSourceKind::Presentation);
    }

    #[test]
    fn a_browser_session_is_a_screen_capture() {
        // 12 slides of about 800 characters, many URL-like lines.
        let decision = classify(12, 12 * 800, 12 * 10, 12 * 5, 12, 15.0 * 60.0);
        assert_eq!(decision.kind, SlideSourceKind::ScreenCapture);
    }

    #[test]
    fn rapid_changes_with_moderate_text_are_a_screen_capture_only_when_fast() {
        // 30 occurrences in 3 minutes, 500 characters per slide.
        let fast = classify(30, 30 * 500, 30 * 10, 0, 30, 180.0);
        assert_eq!(fast.kind, SlideSourceKind::ScreenCapture);

        // The same slides over an hour change slowly and look like a deck.
        let slow = classify(30, 30 * 500, 30 * 10, 0, 30, 3_600.0);
        assert_eq!(slow.kind, SlideSourceKind::Presentation);
    }

    #[test]
    fn no_slides_is_a_presentation_without_panicking() {
        let decision = classify(0, 0, 0, 0, 0, 0.0);
        assert_eq!(decision.kind, SlideSourceKind::Presentation);
        assert_eq!(decision.reason, "no slides");

        // An unknown duration never triggers the rate rule, however many changes.
        let unknown = classify(30, 30 * 500, 30 * 10, 0, 1_000, 0.0);
        assert_eq!(unknown.kind, SlideSourceKind::Presentation);
    }

    #[test]
    fn density_and_chrome_thresholds_are_inclusive_at_the_limit() {
        // Rule 1: average density of at least 1,200 characters.
        assert_eq!(
            classify(5, 6_000, 0, 0, 0, 60.0).kind,
            SlideSourceKind::ScreenCapture
        );
        assert_eq!(
            classify(5, 5_995, 0, 0, 0, 60.0).kind,
            SlideSourceKind::Presentation
        );

        // Rule 2: at least 700 characters with at least 25% chrome-like lines.
        assert_eq!(
            classify(10, 7_000, 100, 25, 10, 600.0).kind,
            SlideSourceKind::ScreenCapture
        );
        assert_eq!(
            classify(10, 7_000, 100, 24, 10, 600.0).kind,
            SlideSourceKind::Presentation
        );
        assert_eq!(
            classify(10, 6_990, 100, 90, 10, 600.0).kind,
            SlideSourceKind::Presentation
        );
    }

    #[test]
    fn the_rate_threshold_is_inclusive_at_the_limit() {
        // Exactly 8 changes per minute with 400 average characters.
        assert_eq!(
            classify(8, 8 * 400, 8, 0, 8, 60.0).kind,
            SlideSourceKind::ScreenCapture
        );
        // Just below the rate limit.
        assert_eq!(
            classify(7, 7 * 400, 7, 0, 7, 60.0).kind,
            SlideSourceKind::Presentation
        );
        // Fast, but too little text per slide for the rate rule.
        assert_eq!(
            classify(8, 8 * 399, 8, 0, 8, 60.0).kind,
            SlideSourceKind::Presentation
        );
    }

    #[test]
    fn chrome_like_lines_cover_urls_paths_prompts_and_code_files() {
        for line in [
            "https://example.com/docs",
            "see www.example.com for details",
            "/usr/local/bin/tool",
            "~/projects/locus",
            "$ cargo test",
            "% git status",
            "> output redirected",
            "C:\\Users\\ana\\notes.txt",
            "C:/Users/ana/notes.txt",
            "main.rs",
            "config.toml",
            "panel.tsx,",
        ] {
            assert!(is_chrome_like(line), "{line:?} should look like chrome");
        }
        for line in [
            "The team agreed on the quarterly plan",
            "Revenue grew by eight percent",
            "10/12 attendees voted yes",
            "Q3 results look strong",
        ] {
            assert!(!is_chrome_like(line), "{line:?} is prose");
        }
    }

    #[test]
    fn stats_are_built_from_raw_slide_rows() {
        let slides = vec![
            slide("a", 1.0, "https://one.example\nplain prose line"),
            slide("b", 2.0, "$ run tests\nmain.rs"),
        ];

        let stats = build_stats(&slides, 7, 42.0);

        assert_eq!(stats.unique_slides, 2);
        assert_eq!(
            stats.total_chars,
            slides
                .iter()
                .map(|slide| slide.ocr_text.chars().count())
                .sum::<usize>()
        );
        assert_eq!(
            stats.max_chars,
            slides
                .iter()
                .map(|slide| slide.ocr_text.chars().count())
                .max()
                .unwrap()
        );
        assert_eq!(stats.total_lines, 4);
        assert_eq!(stats.chrome_like_lines, 3);
        assert_eq!(stats.occurrences, 7);
        assert_eq!(stats.duration_seconds, 42.0);
    }

    #[test]
    fn a_negative_duration_is_treated_as_unknown() {
        let stats = build_stats(&[], 0, -5.0);
        assert_eq!(stats.duration_seconds, 0.0);
    }

    #[test]
    fn empty_rows_do_not_panic_or_divide_by_zero() {
        let stats = build_stats(&[slide("a", 0.0, "")], 0, 0.0);
        assert_eq!(stats.unique_slides, 1);
        assert_eq!(stats.total_chars, 0);
        assert_eq!(stats.total_lines, 0);
        let decision = classify_slide_source(&stats);
        assert_eq!(decision.kind, SlideSourceKind::Presentation);
    }
}
