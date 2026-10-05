//! Video frame type shared by every platform adapter and the encoder
//! (NATIVE_CAPTURE_PLAN §4.1).

use std::{borrow::Cow, sync::Arc, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// Planar Y plane followed by an interleaved UV plane (preferred: ~93 MB/s at 1080p30).
    Nv12,
    /// 8-bit B, G, R, A (fallback: ~250 MB/s at 1080p30).
    Bgra,
}

impl PixelFormat {
    /// Name FFmpeg expects for `-pix_fmt` on a rawvideo input.
    pub fn ffmpeg_name(self) -> &'static str {
        match self {
            Self::Nv12 => "nv12",
            Self::Bgra => "bgra",
        }
    }

    /// Bytes in one row of the first plane without padding.
    pub fn row_bytes(self, width: u32) -> usize {
        match self {
            Self::Nv12 => width as usize,
            Self::Bgra => width as usize * 4,
        }
    }

    /// Rows across all planes (NV12's UV plane adds half the height).
    pub fn row_count(self, height: u32) -> usize {
        match self {
            Self::Nv12 => height as usize + height as usize / 2,
            Self::Bgra => height as usize,
        }
    }

    /// Size of one tightly packed frame.
    pub fn packed_len(self, width: u32, height: u32) -> usize {
        self.row_bytes(width) * self.row_count(height)
    }
}

/// One captured picture. `pts` is on the shared media clock (pause time
/// removed). The pixel buffer is reference counted so the pacer can repeat a
/// frame for an idle screen without copying it.
#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub data: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    /// Bytes per row (of the Y plane for NV12; the UV plane uses the same stride).
    pub stride: u32,
    pub pixel_format: PixelFormat,
    pub pts: Duration,
}

impl VideoFrame {
    /// A tightly packed frame (`stride` equals the natural row size).
    pub fn packed(
        data: Vec<u8>,
        width: u32,
        height: u32,
        pixel_format: PixelFormat,
        pts: Duration,
    ) -> Self {
        Self {
            data: Arc::new(data),
            width,
            height,
            stride: pixel_format.row_bytes(width) as u32,
            pixel_format,
            pts,
        }
    }

    pub fn with_pts(&self, pts: Duration) -> Self {
        Self {
            pts,
            ..self.clone()
        }
    }

    /// H.264 4:2:0 needs even dimensions; the buffer must hold every row.
    pub fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 || self.width % 2 != 0 || self.height % 2 != 0 {
            return Err(format!(
                "frame size {}x{} must be non-zero and even",
                self.width, self.height
            ));
        }
        let row = self.pixel_format.row_bytes(self.width);
        let stride = self.stride as usize;
        if stride < row {
            return Err(format!("stride {stride} is smaller than a row ({row})"));
        }
        let rows = self.pixel_format.row_count(self.height);
        let needed = stride * (rows - 1) + row;
        if self.data.len() < needed {
            return Err(format!(
                "buffer holds {} bytes but {needed} are needed",
                self.data.len()
            ));
        }
        Ok(())
    }

    /// Pixel data without row padding, as FFmpeg's rawvideo input expects.
    pub fn packed_bytes(&self) -> Cow<'_, [u8]> {
        let row = self.pixel_format.row_bytes(self.width);
        let stride = self.stride as usize;
        let rows = self.pixel_format.row_count(self.height);
        if stride == row {
            return Cow::Borrowed(&self.data[..row * rows]);
        }
        let mut out = Vec::with_capacity(row * rows);
        for r in 0..rows {
            out.extend_from_slice(&self.data[r * stride..r * stride + row]);
        }
        Cow::Owned(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nv12_and_bgra_sizes() {
        assert_eq!(
            PixelFormat::Nv12.packed_len(1920, 1080),
            1920 * 1080 * 3 / 2
        );
        assert_eq!(PixelFormat::Bgra.packed_len(1920, 1080), 1920 * 1080 * 4);
    }

    #[test]
    fn rejects_odd_sizes_and_short_buffers() {
        let odd = VideoFrame::packed(vec![0; 100], 5, 4, PixelFormat::Nv12, Duration::ZERO);
        assert!(odd.validate().is_err());
        let short = VideoFrame::packed(vec![0; 10], 4, 4, PixelFormat::Nv12, Duration::ZERO);
        assert!(short.validate().is_err());
        let ok = VideoFrame::packed(vec![0; 24], 4, 4, PixelFormat::Nv12, Duration::ZERO);
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn padded_stride_is_removed() {
        // 4x2 NV12 with an 8-byte stride: 3 rows (2 Y + 1 UV), 4 useful bytes each.
        let mut data = Vec::new();
        for row in 0..3u8 {
            data.extend_from_slice(&[row; 4]);
            data.extend_from_slice(&[0xEE; 4]);
        }
        let frame = VideoFrame {
            data: Arc::new(data),
            width: 4,
            height: 2,
            stride: 8,
            pixel_format: PixelFormat::Nv12,
            pts: Duration::ZERO,
        };
        assert!(frame.validate().is_ok());
        assert_eq!(
            frame.packed_bytes().as_ref(),
            &[0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2]
        );
    }
}
