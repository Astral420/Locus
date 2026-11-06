//! Turns raw per-frame OCR into compact, chronological source text for the
//! summary prompt.
//!
//! Screen recordings produce one OCR row per changed frame, so most of the
//! text is browser chrome, terminal output and repeated menu bars. The rules
//! here are deliberately language-agnostic: they only use character classes
//! and frequency, never English word lists. The stored OCR rows are never
//! modified; this is a prompt-side projection only.

use std::collections::{HashMap, HashSet};

use super::summarization::SlideRow;

#[derive(Debug, Clone, PartialEq)]
pub struct CleanedSlide {
    pub id: String,
    pub timestamp_seconds: f32,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanStats {
    pub slides_in: usize,
    pub slides_out: usize,
    pub lines_in: usize,
    pub lines_out: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CleanedSlides {
    pub slides: Vec<CleanedSlide>,
    pub stats: CleanStats,
}

struct WorkingLine {
    text: String,
    key: String,
}

struct WorkingSlide {
    id: String,
    timestamp_seconds: f32,
    lines: Vec<WorkingLine>,
    signature: HashSet<String>,
}

/// Cleans OCR rows in the order given. Callers pass chronological order so
/// the output keeps the meeting's timeline and near-duplicate merging only
/// ever compares consecutive slides.
pub fn clean_slides(slides: &[SlideRow]) -> CleanedSlides {
    let slides_in = slides.len();
    let lines_in = slides
        .iter()
        .map(|slide| slide.ocr_text.lines().count())
        .sum();
    let mut working = slides
        .iter()
        .map(|slide| {
            let lines = slide
                .ocr_text
                .lines()
                .map(normalize_line)
                .filter(|line| line.chars().filter(|c| c.is_alphabetic()).count() >= 2)
                .map(|line| WorkingLine {
                    key: comparison_key(&line),
                    text: line,
                })
                .collect::<Vec<_>>();
            WorkingSlide {
                id: slide.id.clone(),
                timestamp_seconds: slide.timestamp_seconds,
                lines,
                signature: HashSet::new(),
            }
        })
        .collect::<Vec<_>>();

    // Chrome is recognised by how many slides a line key appears on. It is
    // dropped before any per-slide bookkeeping so it can never become a
    // "kept first occurrence" or a near-duplicate signal.
    let mut slide_counts: HashMap<String, usize> = HashMap::new();
    for slide in &working {
        let mut on_this_slide = HashSet::new();
        for line in &slide.lines {
            if on_this_slide.insert(line.key.clone()) {
                *slide_counts.entry(line.key.clone()).or_insert(0) += 1;
            }
        }
    }
    for slide in &mut working {
        slide.lines.retain(|line| {
            let count = slide_counts.get(&line.key).copied().unwrap_or_default();
            !is_chrome(count, slides_in)
        });
        slide.signature = slide.lines.iter().map(|line| line.key.clone()).collect();
    }

    // Cross-slide duplicates keep only their first occurrence. Near-duplicate
    // detection compares the signatures captured before this dedupe; the
    // shared lines of a near-duplicate slide are already dropped here, so
    // comparing surviving lines would hide the very similarity we look for.
    let mut seen: HashSet<String> = HashSet::new();
    let mut kept: Vec<CleanedSlide> = Vec::new();
    let mut kept_signatures: Vec<HashSet<String>> = Vec::new();
    let mut previous: Option<usize> = None;
    for slide in working {
        let mut lines = Vec::new();
        for line in slide.lines {
            if seen.insert(line.key) {
                lines.push(line.text);
            }
        }
        if lines.is_empty() {
            continue;
        }
        if let Some(index) = previous {
            if similar(&kept_signatures[index], &slide.signature) {
                kept[index].lines.extend(lines);
                kept_signatures[index] = slide.signature;
                continue;
            }
        }
        let index = kept.len();
        kept.push(CleanedSlide {
            id: slide.id,
            timestamp_seconds: slide.timestamp_seconds,
            lines,
        });
        kept_signatures.push(slide.signature);
        previous = Some(index);
    }

    CleanedSlides {
        stats: CleanStats {
            slides_in,
            slides_out: kept.len(),
            lines_in,
            lines_out: kept.iter().map(|slide| slide.lines.len()).sum(),
        },
        slides: kept,
    }
}

fn normalize_line(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn comparison_key(line: &str) -> String {
    let mut key = String::new();
    let mut pending_space = false;
    for character in line.chars() {
        if character.is_whitespace() {
            pending_space = !key.is_empty();
            continue;
        }
        if pending_space {
            key.push(' ');
            pending_space = false;
        }
        if character.is_numeric() {
            key.push('#');
        } else {
            key.extend(character.to_lowercase());
        }
    }
    key
}

fn is_chrome(count: usize, slides_in: usize) -> bool {
    count >= 3 || (slides_in >= 3 && count * 5 >= slides_in * 2)
}

fn similar(previous: &HashSet<String>, current: &HashSet<String>) -> bool {
    if previous.is_empty() || current.is_empty() {
        return false;
    }
    let intersection = previous.intersection(current).count();
    let union = previous.len() + current.len() - intersection;
    intersection * 5 >= union * 4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slide<S: AsRef<str>>(id: &str, timestamp_seconds: f32, lines: &[S]) -> SlideRow {
        SlideRow {
            id: id.to_owned(),
            timestamp_seconds,
            ocr_text: lines
                .iter()
                .map(|line| line.as_ref())
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    fn kept_lines(cleaned: &CleanedSlides) -> Vec<String> {
        cleaned
            .slides
            .iter()
            .flat_map(|slide| slide.lines.iter().cloned())
            .collect()
    }

    const CHROME: [&str; 6] = [
        "File Edit View History Bookmarks Tab",
        "Chrome menu extensions settings",
        "localhost port development server",
        "terminal prompt achilles project",
        "downloads folder recent files",
        "menu bar wifi battery volume",
    ];

    #[test]
    fn repeated_chrome_disappears_from_every_slide() {
        let markers = ["alpha", "bravo", "charlie", "delta", "echo"];
        let slides = (0..5)
            .map(|index| {
                let mut lines = CHROME
                    .iter()
                    .map(|line| line.to_string())
                    .collect::<Vec<_>>();
                lines.push(format!(
                    "{marker} unique content on this slide",
                    marker = markers[index]
                ));
                slide(&format!("s{index}"), index as f32, &lines)
            })
            .collect::<Vec<_>>();

        let cleaned = clean_slides(&slides);

        assert_eq!(cleaned.stats.slides_in, 5);
        assert_eq!(cleaned.stats.slides_out, 5);
        assert_eq!(cleaned.stats.lines_in, 35);
        assert_eq!(cleaned.stats.lines_out, 5);
        for (index, slide) in cleaned.slides.iter().enumerate() {
            assert_eq!(slide.lines.len(), 1);
            assert!(slide.lines[0].contains(markers[index]));
        }
    }

    #[test]
    fn a_cross_slide_duplicate_keeps_only_the_first_occurrence() {
        let slides = vec![
            slide(
                "first",
                0.0,
                &["Team agreed on the launch date", "Budget was approved"],
            ),
            slide(
                "second",
                4.0,
                &["Team agreed on the launch date", "Hiring starts in spring"],
            ),
        ];

        let cleaned = clean_slides(&slides);

        assert_eq!(cleaned.slides.len(), 2);
        assert!(cleaned.slides[0]
            .lines
            .contains(&"Team agreed on the launch date".to_owned()));
        assert!(!cleaned.slides[1]
            .lines
            .contains(&"Team agreed on the launch date".to_owned()));
        assert_eq!(cleaned.stats.lines_out, 3);
    }

    #[test]
    fn digit_variants_compare_equal_and_clock_like_lines_vanish() {
        let slides = vec![
            slide("s0", 0.0, &["Status meeting starts 10:31"]),
            slide("s1", 0.0, &["Status meeting starts 10:32"]),
        ];
        let cleaned = clean_slides(&slides);
        assert_eq!(cleaned.slides.len(), 1);
        assert_eq!(cleaned.slides[0].lines, vec!["Status meeting starts 10:31"]);

        let pure_clock = (0..5)
            .map(|index| {
                slide(
                    &format!("c{index}"),
                    0.0,
                    &[
                        format!("10:3{index}"),
                        "Agenda reviewed by the chair".to_owned(),
                    ],
                )
            })
            .collect::<Vec<_>>();
        let cleaned = clean_slides(&pure_clock);
        assert!(cleaned.slides.is_empty());
    }

    #[test]
    fn near_duplicate_consecutive_slides_merge_but_dissimilar_ones_do_not() {
        let mut first = vec![
            "Agenda item alpha",
            "Agenda item beta",
            "Agenda item gamma",
            "Agenda item delta",
            "Agenda item epsilon",
            "Agenda item zeta",
            "Agenda item eta",
            "Agenda item theta",
            "Agenda item iota",
        ];
        first.push("Final closing remarks");
        let mut second = first[..9].to_vec();
        second.push("A newly added discussion point");
        let slides = vec![slide("first", 5.0, &first), slide("second", 9.0, &second)];

        let cleaned = clean_slides(&slides);

        assert_eq!(cleaned.stats.slides_out, 1);
        assert_eq!(cleaned.slides[0].id, "first");
        assert_eq!(cleaned.slides[0].timestamp_seconds, 5.0);
        assert_eq!(cleaned.slides[0].lines.len(), 11);
        assert!(cleaned.slides[0]
            .lines
            .contains(&"A newly added discussion point".to_owned()));

        let dissimilar = vec![
            slide(
                "a",
                0.0,
                &[
                    "Revenue grew by eight percent",
                    "North region leads the quarter",
                ],
            ),
            slide(
                "b",
                3.0,
                &["Design review scheduled", "New hire onboarding next week"],
            ),
        ];
        let cleaned = clean_slides(&dissimilar);
        assert_eq!(cleaned.stats.slides_out, 2);
    }

    #[test]
    fn multilingual_lines_survive_intact() {
        let slides = vec![
            slide("tl", 0.0, &["Magandang umaga sa lahat ng dumalo"]),
            slide("ja", 3.0, &["来期の売上目標を確認します"]),
            slide("ar", 6.0, &["نناقش خطة التسويق الجديدة"]),
            slide("fr", 9.0, &["Résumé de la réunion à Genève"]),
        ];

        let cleaned = clean_slides(&slides);

        assert_eq!(cleaned.stats.lines_out, 4);
        let lines = kept_lines(&cleaned);
        for expected in [
            "Magandang umaga sa lahat ng dumalo",
            "来期の売上目標を確認します",
            "نناقش خطة التسويق الجديدة",
            "Résumé de la réunion à Genève",
        ] {
            assert!(lines.contains(&expected.to_owned()), "missing {expected:?}");
        }
    }

    #[test]
    fn cleaning_clean_data_is_idempotent_and_empty_input_stays_empty() {
        let slides = vec![
            slide("one", 0.0, &["Alpha topic discussed by the team"]),
            slide("two", 4.0, &["Beta topic reviewed by the team"]),
            slide("three", 8.0, &["Gamma topic approved by the team"]),
        ];
        let first = clean_slides(&slides);
        let rebuilt = first
            .slides
            .iter()
            .map(|slide| SlideRow {
                id: slide.id.clone(),
                timestamp_seconds: slide.timestamp_seconds,
                ocr_text: slide.lines.join("\n"),
            })
            .collect::<Vec<_>>();
        let second = clean_slides(&rebuilt);
        assert_eq!(first.slides, second.slides);
        assert_eq!(first.stats.lines_out, second.stats.lines_out);

        let empty = clean_slides(&[]);
        assert!(empty.slides.is_empty());
        assert_eq!(
            empty.stats,
            CleanStats {
                slides_in: 0,
                slides_out: 0,
                lines_in: 0,
                lines_out: 0,
            }
        );
    }

    #[test]
    fn browser_heavy_slides_shrink_substantially() {
        const TOPICS: [&str; 40] = [
            "overview",
            "metrics",
            "roadmap",
            "backlog",
            "sprint",
            "review",
            "design",
            "research",
            "support",
            "billing",
            "onboarding",
            "retention",
            "growth",
            "launch",
            "release",
            "quality",
            "security",
            "privacy",
            "infrastructure",
            "monitoring",
            "performance",
            "latency",
            "storage",
            "network",
            "deployment",
            "rollback",
            "incident",
            "recovery",
            "archive",
            "search",
            "indexing",
            "import",
            "export",
            "settings",
            "profile",
            "notifications",
            "messages",
            "calendar",
            "meetings",
            "notes",
        ];
        let slides = (0..5)
            .map(|index| {
                let marker = ["alpha", "bravo", "charlie", "delta", "echo"][index];
                let mut lines = CHROME
                    .iter()
                    .map(|line| line.to_string())
                    .collect::<Vec<_>>();
                for topic in TOPICS {
                    lines.push(format!(
                        "{marker} browser panel row {topic} shows project status and notes for the team"
                    ));
                }
                let text = lines.join("\n");
                assert!(text.chars().count() >= 2_300);
                slide(&format!("s{index}"), index as f32, &lines)
            })
            .collect::<Vec<_>>();

        let cleaned = clean_slides(&slides);

        assert!(cleaned.stats.lines_out < cleaned.stats.lines_in);
        assert_eq!(cleaned.stats.lines_out, 5 * TOPICS.len());
    }
}
