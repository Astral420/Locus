//! Windows capture: preflight and the pure logic behind the Windows Graphics
//! Capture adapter (NATIVE_CAPTURE_PLAN NC-6). The OS-facing adapter is
//! `windows_wgc.rs` (Windows only); everything here compiles and is tested on
//! every platform.

use super::{CaptureBackend, CaptureError, CaptureOptions};

/// First Windows 10 build with Windows Graphics Capture (version 1903).
pub const MIN_WGC_BUILD: u32 = 18362;

/// The build number from `cmd /C ver`, e.g.
/// `Microsoft Windows [Version 10.0.19045.3693]` gives 19045.
pub fn parse_windows_build(text: &str) -> Option<u32> {
    let version = text.split("Version").nth(1)?;
    let version = version.trim_start().trim_start_matches([' ', ':']);
    let version: String = version
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = version.split('.');
    let (_major, _minor) = (parts.next()?, parts.next()?);
    parts.next()?.parse().ok()
}

pub fn build_supports_wgc(build: u32) -> bool {
    build >= MIN_WGC_BUILD
}

/// Averages `src` (BGRA, `src_stride` bytes per row) down to `dst_w` x
/// `dst_h`, each output pixel being the mean of the source pixels it covers.
/// Sharp on text, and far cheaper to pipe to FFmpeg than a 4K frame. Returns
/// a tightly packed BGRA buffer, or `None` if the buffer is too short.
pub fn scale_bgra(
    src: &[u8],
    src_w: usize,
    src_h: usize,
    src_stride: usize,
    dst_w: usize,
    dst_h: usize,
) -> Option<Vec<u8>> {
    if src_w == 0 || src_h == 0 || dst_w == 0 || dst_h == 0 || src_stride < src_w * 4 {
        return None;
    }
    let needed = src_stride.checked_mul(src_h - 1)?.checked_add(src_w * 4)?;
    if src.len() < needed {
        return None;
    }
    let mut out = vec![0u8; dst_w * dst_h * 4];
    if (src_w, src_h) == (dst_w, dst_h) {
        for y in 0..dst_h {
            out[y * dst_w * 4..(y + 1) * dst_w * 4]
                .copy_from_slice(&src[y * src_stride..y * src_stride + dst_w * 4]);
        }
        return Some(out);
    }
    // Whole-number ratio (4K -> 1080p is exactly 2): every output pixel covers
    // the same block, so the inner loop needs no per-pixel bookkeeping.
    if src_w % dst_w == 0 && src_h % dst_h == 0 {
        let (kx, ky) = (src_w / dst_w, src_h / dst_h);
        let count = (kx * ky) as u32;
        let reciprocal = ((1u64 << 32) + u64::from(count) - 1) / u64::from(count);
        let mut sums = vec![0u32; src_w * 4];
        for y in 0..dst_h {
            sums.iter_mut().for_each(|s| *s = 0);
            for row in y * ky..(y + 1) * ky {
                let line = &src[row * src_stride..row * src_stride + src_w * 4];
                for (sum, &byte) in sums.iter_mut().zip(line) {
                    *sum += u32::from(byte);
                }
            }
            let out_row = &mut out[y * dst_w * 4..(y + 1) * dst_w * 4];
            for (pixel, block) in out_row.chunks_exact_mut(4).zip(sums.chunks_exact(kx * 4)) {
                let mut total = [0u32; 4];
                for source in block.chunks_exact(4) {
                    total[0] += source[0];
                    total[1] += source[1];
                    total[2] += source[2];
                    total[3] += source[3];
                }
                for (byte, total) in pixel.iter_mut().zip(total) {
                    *byte = (((u64::from(total) + u64::from(count / 2)) * reciprocal) >> 32) as u8;
                }
            }
        }
        return Some(out);
    }
    // Source columns covered by each output column (computed once).
    let columns: Vec<(usize, usize)> = (0..dst_w)
        .map(|x| {
            let start = x * src_w / dst_w;
            let end = ((x + 1) * src_w).div_ceil(dst_w).clamp(start + 1, src_w);
            (start, end)
        })
        .collect();
    let mut sums = vec![0u32; src_w * 4];
    // Reciprocals per output column for each distinct row count (at most two
    // occur), so the inner loop has no division.
    let mut tables: Vec<(u64, Vec<u64>)> = Vec::new();
    for y in 0..dst_h {
        let row_start = y * src_h / dst_h;
        let row_end = ((y + 1) * src_h)
            .div_ceil(dst_h)
            .clamp(row_start + 1, src_h);
        sums.iter_mut().for_each(|s| *s = 0);
        for row in row_start..row_end {
            let line = &src[row * src_stride..row * src_stride + src_w * 4];
            for (sum, &byte) in sums.iter_mut().zip(line) {
                *sum += u32::from(byte);
            }
        }
        let rows = (row_end - row_start) as u64;
        let table = match tables.iter().position(|(r, _)| *r == rows) {
            Some(index) => index,
            None => {
                let reciprocals = columns
                    .iter()
                    .map(|&(start, end)| {
                        let count = rows * (end - start) as u64;
                        ((1u64 << 32) + count - 1) / count
                    })
                    .collect();
                tables.push((rows, reciprocals));
                tables.len() - 1
            }
        };
        let reciprocals = &tables[table].1;
        let out_row = &mut out[y * dst_w * 4..(y + 1) * dst_w * 4];
        for ((pixel, &(col_start, col_end)), &reciprocal) in
            out_row.chunks_exact_mut(4).zip(&columns).zip(reciprocals)
        {
            let mut total = [0u32; 4];
            for source in sums[col_start * 4..col_end * 4].chunks_exact(4) {
                total[0] += source[0];
                total[1] += source[1];
                total[2] += source[2];
                total[3] += source[3];
            }
            let half = rows * (col_end - col_start) as u64 / 2;
            for (byte, total) in pixel.iter_mut().zip(total) {
                *byte = (((u64::from(total) + half) * reciprocal) >> 32) as u8;
            }
        }
    }
    Some(out)
}

/// Turns a Windows Graphics Capture failure into an actionable message.
pub fn describe_capture_failure(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("unsupported") || lower.contains("not supported") {
        return "Screen capture needs Windows 10 version 1903 (build 18362) or later.".into();
    }
    if lower.contains("access is denied") || lower.contains("0x80070005") {
        return "Windows blocked screen capture for Locus. Check Settings > Privacy & security > \
                Screen capture (or Graphics capture), allow desktop apps, then try again."
            .into();
    }
    format!("Screen capture failed: {raw}")
}

pub fn preflight(options: &CaptureOptions) -> Result<(), CaptureError> {
    #[cfg(target_os = "windows")]
    {
        use crate::contracts::CaptureSource;
        if std::env::var("LOCUS_WINDOWS_CAPTURE_TEST").ok().as_deref() == Some("1") {
            return Ok(());
        }
        if std::env::var("WINDIR").is_err() {
            return Err(CaptureError::SourceUnavailable(
                "Windows Graphics Capture requires a Windows session".into(),
            ));
        }
        let native = matches!(CaptureBackend::from_env(), Ok(CaptureBackend::Native));
        if native && options.sources.contains(&CaptureSource::Screen) {
            let text = std::process::Command::new("cmd")
                .args(["/C", "ver"])
                .output()
                .ok()
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .unwrap_or_default();
            // An unreadable version is let through: starting capture reports
            // the real error if the OS lacks the API.
            if let Some(build) = parse_windows_build(&text) {
                if !build_supports_wgc(build) {
                    return Err(CaptureError::SourceUnavailable(format!(
                        "Screen capture needs Windows 10 version 1903 or later (this PC is build {build})."
                    )));
                }
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (options, CaptureBackend::Ffmpeg);
        Err(CaptureError::SourceUnavailable(
            "Windows Graphics Capture is only available on Windows builds".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_build_is_read_from_ver_output() {
        assert_eq!(
            parse_windows_build("\r\nMicrosoft Windows [Version 10.0.19045.3693]\r\n"),
            Some(19045)
        );
        assert_eq!(
            parse_windows_build("Microsoft Windows [Version 10.0.26100.1]"),
            Some(26100)
        );
        assert_eq!(parse_windows_build("garbage"), None);
        assert_eq!(parse_windows_build(""), None);
    }

    #[test]
    fn graphics_capture_needs_windows_10_1903() {
        assert!(!build_supports_wgc(17763)); // 1809
        assert!(build_supports_wgc(18362)); // 1903
        assert!(build_supports_wgc(22631)); // Windows 11
    }

    fn solid(w: usize, h: usize, stride: usize, bgra: [u8; 4]) -> Vec<u8> {
        let mut data = vec![0xAA; stride * h];
        for y in 0..h {
            for x in 0..w {
                data[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&bgra);
            }
        }
        data
    }

    #[test]
    fn same_size_copy_drops_row_padding() {
        let src = solid(4, 2, 24, [1, 2, 3, 255]);
        let out = scale_bgra(&src, 4, 2, 24, 4, 2).unwrap();
        assert_eq!(out.len(), 4 * 2 * 4);
        assert!(out.chunks(4).all(|p| p == [1, 2, 3, 255]));
    }

    #[test]
    fn a_solid_colour_survives_any_scale() {
        for (sw, sh, dw, dh) in [
            (3840, 2160, 1920, 1080),
            (2560, 1440, 1920, 1080),
            (101, 77, 50, 38),
            (10, 10, 3, 7),
        ] {
            let src = solid(sw, sh, sw * 4 + 8, [10, 200, 90, 255]);
            let out = scale_bgra(&src, sw, sh, sw * 4 + 8, dw, dh).unwrap();
            assert_eq!(out.len(), dw * dh * 4);
            assert!(
                out.chunks(4).all(|p| p == [10, 200, 90, 255]),
                "{sw}x{sh} to {dw}x{dh}"
            );
        }
    }

    #[test]
    fn two_by_two_blocks_are_averaged() {
        // 4x2 source -> 2x1: left block is black/white, right block all 100.
        let mut src = vec![0u8; 4 * 2 * 4];
        let set = |src: &mut Vec<u8>, x: usize, y: usize, v: u8| {
            src[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4].copy_from_slice(&[v, v, v, 255]);
        };
        set(&mut src, 0, 0, 0);
        set(&mut src, 1, 0, 255);
        set(&mut src, 0, 1, 255);
        set(&mut src, 1, 1, 0);
        for (x, y) in [(2, 0), (3, 0), (2, 1), (3, 1)] {
            set(&mut src, x, y, 100);
        }
        let out = scale_bgra(&src, 4, 2, 16, 2, 1).unwrap();
        assert_eq!(&out[0..4], &[128, 128, 128, 255]); // (0+255+255+0)/4 rounded
        assert_eq!(&out[4..8], &[100, 100, 100, 255]);
    }

    /// Straightforward reference: exact rounded mean of every covered pixel.
    fn reference_scale(
        src: &[u8],
        sw: usize,
        sh: usize,
        stride: usize,
        dw: usize,
        dh: usize,
    ) -> Vec<u8> {
        let mut out = vec![0u8; dw * dh * 4];
        for y in 0..dh {
            let (r0, r1) = (
                y * sh / dh,
                ((y + 1) * sh).div_ceil(dh).clamp(y * sh / dh + 1, sh),
            );
            for x in 0..dw {
                let (c0, c1) = (
                    x * sw / dw,
                    ((x + 1) * sw).div_ceil(dw).clamp(x * sw / dw + 1, sw),
                );
                let count = ((r1 - r0) * (c1 - c0)) as u32;
                for ch in 0..4 {
                    let mut total = 0u32;
                    for r in r0..r1 {
                        for c in c0..c1 {
                            total += u32::from(src[r * stride + c * 4 + ch]);
                        }
                    }
                    out[(y * dw + x) * 4 + ch] = ((total + count / 2) / count) as u8;
                }
            }
        }
        out
    }

    #[test]
    fn scaling_matches_an_exact_reference_on_noise() {
        // Simple deterministic noise; whole-number and fractional ratios.
        let mut seed = 12345u32;
        let mut noise = |n: usize| -> Vec<u8> {
            (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    (seed >> 24) as u8
                })
                .collect()
        };
        for (sw, sh, dw, dh) in [
            (64, 48, 32, 24),
            (64, 48, 48, 36),
            (50, 30, 20, 17),
            (37, 41, 11, 5),
            (9, 9, 4, 4),
        ] {
            let stride = sw * 4 + 12;
            let src = noise(stride * sh);
            let fast = scale_bgra(&src, sw, sh, stride, dw, dh).unwrap();
            let slow = reference_scale(&src, sw, sh, stride, dw, dh);
            let worst = fast
                .iter()
                .zip(&slow)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(worst <= 1, "{sw}x{sh} to {dw}x{dh}: worst error {worst}");
        }
    }

    #[test]
    fn a_vertical_edge_stays_sharp_at_half_scale() {
        // Left half white, right half black, 8x4 -> 4x2: columns stay pure.
        let mut src = vec![0u8; 8 * 4 * 4];
        for y in 0..4 {
            for x in 0..4 {
                src[(y * 8 + x) * 4..(y * 8 + x) * 4 + 4].copy_from_slice(&[255; 4]);
            }
        }
        let out = scale_bgra(&src, 8, 4, 32, 4, 2).unwrap();
        for y in 0..2 {
            assert_eq!(out[(y * 4) * 4], 255);
            assert_eq!(out[(y * 4 + 1) * 4], 255);
            assert_eq!(out[(y * 4 + 2) * 4], 0);
            assert_eq!(out[(y * 4 + 3) * 4], 0);
        }
    }

    #[test]
    fn short_or_degenerate_buffers_are_rejected() {
        assert!(scale_bgra(&[0; 10], 4, 4, 16, 2, 2).is_none());
        assert!(scale_bgra(&[0; 64], 0, 4, 16, 2, 2).is_none());
        assert!(
            scale_bgra(&[0; 64], 4, 4, 8, 2, 2).is_none(),
            "stride below one row"
        );
        assert!(scale_bgra(&[0; 64], 4, 4, 16, 0, 2).is_none());
    }

    #[test]
    fn full_hd_frame_is_scaled_quickly_enough_for_30_fps() {
        // 4K -> 1080p must fit comfortably inside one 33 ms frame in release;
        // here only a generous ceiling so a debug build does not fail.
        let src = solid(3840, 2160, 3840 * 4, [1, 2, 3, 255]);
        let started = std::time::Instant::now();
        let out = scale_bgra(&src, 3840, 2160, 3840 * 4, 1920, 1080).unwrap();
        let elapsed = started.elapsed();
        assert_eq!(out.len(), 1920 * 1080 * 4);
        assert!(elapsed.as_secs_f64() < 5.0, "{elapsed:?}");
        eprintln!("scale_bgra 4K->1080p: {elapsed:?} (this build profile)");
    }

    #[test]
    fn capture_failures_get_actionable_messages() {
        assert!(describe_capture_failure("Unsupported").contains("1903"));
        assert!(describe_capture_failure("Access is denied. (0x80070005)").contains("Privacy"));
        assert!(describe_capture_failure("display lost").starts_with("Screen capture failed"));
    }
}
