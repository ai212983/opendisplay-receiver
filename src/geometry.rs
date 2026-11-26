//! Frame dimensions, pixel formats, and wire alignment.
//!
//! Each wire plane is row-major and MSB-first. Rows consist of whole bytes
//! and are stored contiguously. The display adapter maps wire coordinates
//! and shade indices to the physical panel.

/// Wire pixel format: packs rows with no padding and selects the Flex
/// `DisplayConfig` `color_scheme` advertisement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// 1 bit per pixel; `color_scheme` 0 (MONO).
    Monochrome,
    /// Two sequential 1-bpp planes, row-major MSB-first
    /// (matching `encodeCanvasToByteData` scheme 5 in the stock browser and
    /// Python clients); `color_scheme` 5 (GRAYSCALE_4).
    Grayscale4,
}

impl PixelFormat {
    /// Sequential 1-bpp planes per frame: 1 for MONO, 2 for Grayscale4.
    pub const fn planes(self) -> u32 {
        match self {
            PixelFormat::Monochrome => 1,
            PixelFormat::Grayscale4 => 2,
        }
    }

    /// Pixels packed per frame byte: 8 for MONO, 4 for Grayscale4
    /// (two planes of 1 bit per pixel).
    pub const fn pixels_per_byte(self) -> u32 {
        match self {
            PixelFormat::Monochrome => 8,
            PixelFormat::Grayscale4 => 4,
        }
    }

    /// Flex `DisplayConfig.color_scheme` wire value. Matches
    /// `enum ColorScheme` in `opendisplay-protocol` (mirrored by
    /// `ColorScheme` in py-opendisplay `models/enums.py`): MONO = 0,
    /// GRAYSCALE_4 = 5.
    pub const fn color_scheme(self) -> u8 {
        match self {
            PixelFormat::Monochrome => 0,
            PixelFormat::Grayscale4 => 5,
        }
    }
}

/// Pixel extent of one product's panel in the wire's landscape frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayGeometry {
    pub width_px: u32,
    pub height_px: u32,
    pub format: PixelFormat,
}

impl DisplayGeometry {
    /// MONO geometry; kept so existing single-format products read unchanged.
    pub const fn new(width_px: u32, height_px: u32) -> Self {
        Self::with_format(width_px, height_px, PixelFormat::Monochrome)
    }

    pub const fn with_format(width_px: u32, height_px: u32, format: PixelFormat) -> Self {
        Self {
            width_px,
            height_px,
            format,
        }
    }

    /// Raw frame bytes: row-major, no row padding (Grayscale4 sends two
    /// sequential 1-bpp planes, so twice the MONO bytes). Rows are whole
    /// bytes for both formats (width multiple of 8) and extents must fit
    /// the wire's u16 fields; extents are validated before any size
    /// arithmetic runs.
    pub const fn frame_bytes(self) -> Option<usize> {
        if self.width_px == 0
            || self.height_px == 0
            || self.width_px > u16::MAX as u32
            || self.height_px > u16::MAX as u32
            || !self.width_px.is_multiple_of(8)
        {
            return None;
        }
        let pixels = self.width_px as u64 * self.height_px as u64;
        Some((pixels / self.format.pixels_per_byte() as u64) as usize)
    }

    /// Bytes per wire row, counting every plane (Grayscale4 rows span both
    /// 1-bpp planes).
    pub const fn stride_bytes(self) -> Option<usize> {
        if self.frame_bytes().is_none() {
            return None;
        }
        Some((self.width_px as u64 / 8 * self.format.planes() as u64) as usize)
    }

    pub const fn is_valid(self) -> bool {
        self.frame_bytes().is_some()
    }
}

/// One probe bit of a decoded frame: row-major, MSB-first, 1 = white.
/// Out-of-range coordinates read 0 so panel code cannot index out of bounds.
pub fn pixel_bit(frame: &[u8], stride_bytes: usize, x: u32, y: u32) -> u8 {
    if stride_bytes == 0 {
        return 0;
    }
    let row = frame.chunks(stride_bytes).nth(y as usize);
    let Some(row) = row else { return 0 };
    let byte = row.get((x / 8) as usize).copied().unwrap_or(0);
    (byte >> (7 - x % 8)) & 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monochrome_frame_and_stride_sizes() {
        assert_eq!(DisplayGeometry::new(600, 600).frame_bytes(), Some(45_000));
        assert_eq!(DisplayGeometry::new(600, 600).stride_bytes(), Some(75));
        assert_eq!(DisplayGeometry::new(400, 300).frame_bytes(), Some(15_000));
        assert_eq!(DisplayGeometry::new(400, 300).stride_bytes(), Some(50));
        assert_eq!(
            DisplayGeometry::new(600, 600).format,
            PixelFormat::Monochrome
        );
        assert_eq!(
            DisplayGeometry::with_format(600, 600, PixelFormat::Monochrome),
            DisplayGeometry::new(600, 600)
        );
    }

    #[test]
    fn frame_and_stride_sizes_for_both_formats() {
        let mono = DisplayGeometry::with_format(480, 800, PixelFormat::Monochrome);
        assert_eq!(mono.frame_bytes(), Some(48_000));
        assert_eq!(mono.stride_bytes(), Some(60));
        // Two sequential 1-bpp planes: 480 * 800 / 4.
        let gray = DisplayGeometry::with_format(480, 800, PixelFormat::Grayscale4);
        assert_eq!(gray.frame_bytes(), Some(96_000));
        assert_eq!(gray.stride_bytes(), Some(120));
    }

    #[test]
    fn row_alignment_is_per_format() {
        // Both formats pack whole bytes per plane (width multiple of 8);
        // Grayscale4 rows span both planes.
        assert_eq!(DisplayGeometry::new(601, 600).frame_bytes(), None);
        assert_eq!(
            DisplayGeometry::with_format(488, 800, PixelFormat::Grayscale4).frame_bytes(),
            Some(488 * 800 / 4)
        );
        assert_eq!(
            DisplayGeometry::with_format(488, 800, PixelFormat::Grayscale4).stride_bytes(),
            Some(2 * (488 / 8))
        );
        assert_eq!(
            DisplayGeometry::with_format(482, 800, PixelFormat::Grayscale4).frame_bytes(),
            None
        );
        assert_eq!(DisplayGeometry::new(0, 600).frame_bytes(), None);
        assert_eq!(DisplayGeometry::new(70_000, 70_000).frame_bytes(), None);
    }

    #[test]
    fn format_wire_values() {
        assert_eq!(PixelFormat::Monochrome.planes(), 1);
        assert_eq!(PixelFormat::Grayscale4.planes(), 2);
        assert_eq!(PixelFormat::Monochrome.pixels_per_byte(), 8);
        assert_eq!(PixelFormat::Grayscale4.pixels_per_byte(), 4);
        assert_eq!(PixelFormat::Monochrome.color_scheme(), 0);
        assert_eq!(PixelFormat::Grayscale4.color_scheme(), 5);
    }

    #[test]
    fn pixel_bit_mono_layout() {
        // MSB-first probe is format-agnostic row math; lock it for MONO rows.
        let frame = [0b1000_0001u8, 0b0100_0000u8];
        assert_eq!(pixel_bit(&frame, 2, 0, 0), 1);
        assert_eq!(pixel_bit(&frame, 2, 7, 0), 1);
        assert_eq!(pixel_bit(&frame, 2, 9, 0), 1);
        assert_eq!(pixel_bit(&frame, 2, 1, 0), 0);
        assert_eq!(pixel_bit(&frame, 2, 0, 1), 0);
    }
}
