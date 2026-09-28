use serde::{Deserialize, Serialize};

pub const MAX_CHUNK_SECONDS: f32 = 5.0 * 60.0;
const MERGE_GAP_SECONDS: f32 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpeechRegion {
    pub start_seconds: f32,
    pub end_seconds: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TranscriptionChunk {
    pub start_seconds: f32,
    pub end_seconds: f32,
}

pub fn chunk_speech_regions(
    regions: &[SpeechRegion],
    energy: &[f32],
    sample_rate: usize,
) -> Vec<TranscriptionChunk> {
    let mut normalized: Vec<_> = regions
        .iter()
        .copied()
        .filter(|r| r.end_seconds > r.start_seconds)
        .collect();
    normalized.sort_by(|a, b| a.start_seconds.total_cmp(&b.start_seconds));
    let mut chunks = Vec::new();
    let mut current: Option<SpeechRegion> = None;
    for region in normalized {
        let Some(existing) = current else {
            current = Some(region);
            continue;
        };
        if region.start_seconds - existing.end_seconds <= MERGE_GAP_SECONDS
            && existing.end_seconds - existing.start_seconds
                + (region.end_seconds - region.start_seconds)
                <= MAX_CHUNK_SECONDS
        {
            current = Some(SpeechRegion {
                start_seconds: existing.start_seconds,
                end_seconds: region.end_seconds,
            });
        } else {
            emit_region(existing, energy, sample_rate, &mut chunks);
            current = Some(region);
        }
    }
    if let Some(region) = current {
        emit_region(region, energy, sample_rate, &mut chunks);
    }
    chunks
}

fn emit_region(
    region: SpeechRegion,
    energy: &[f32],
    sample_rate: usize,
    output: &mut Vec<TranscriptionChunk>,
) {
    let mut start = region.start_seconds;
    while region.end_seconds - start > MAX_CHUNK_SECONDS {
        let target = start + MAX_CHUNK_SECONDS;
        let split =
            lowest_energy_boundary(target - 30.0, target, energy, sample_rate).unwrap_or(target);
        output.push(TranscriptionChunk {
            start_seconds: start,
            end_seconds: split,
        });
        start = split;
    }
    if region.end_seconds > start {
        output.push(TranscriptionChunk {
            start_seconds: start,
            end_seconds: region.end_seconds,
        });
    }
}

fn lowest_energy_boundary(start: f32, end: f32, energy: &[f32], sample_rate: usize) -> Option<f32> {
    if energy.is_empty() || sample_rate == 0 {
        return None;
    }
    let first = (start * sample_rate as f32) as usize;
    let last = ((end * sample_rate as f32) as usize).min(energy.len().saturating_sub(1));
    (first..=last)
        .min_by(|a, b| energy[*a].total_cmp(&energy[*b]))
        .map(|sample| sample as f32 / sample_rate as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_are_bounded_and_preserve_original_timeline() {
        let energy = vec![0.8; 700];
        let chunks = chunk_speech_regions(
            &[SpeechRegion {
                start_seconds: 0.0,
                end_seconds: 620.0,
            }],
            &energy,
            1,
        );
        assert_eq!(chunks.len(), 3);
        assert!(chunks
            .iter()
            .all(|c| c.end_seconds - c.start_seconds <= MAX_CHUNK_SECONDS));
        assert_eq!(chunks.first().unwrap().start_seconds, 0.0);
        assert_eq!(chunks.last().unwrap().end_seconds, 620.0);
        assert!(chunks
            .windows(2)
            .all(|pair| pair[0].end_seconds <= pair[1].start_seconds));
    }
}
