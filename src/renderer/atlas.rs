use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache};
use std::collections::HashMap;

/// Rasterized glyph data for GPU upload.
pub struct RasterizedGlyph {
    pub width: u32,
    pub height: u32,
    /// RGBA pixel data (width × height × 4).
    pub data: Vec<u8>,
    /// Offset from the cell origin.
    pub left: i32,
    pub top: i32,
}

/// Glyph atlas: caches rasterized glyphs and their positions in a texture.
pub struct GlyphAtlas {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    /// Cell dimensions in pixels.
    pub cell_width: f32,
    pub cell_height: f32,
    /// Font metrics.
    pub font_size: f32,
    pub line_height: f32,
    /// Cache of glyph images keyed by (char, bold, italic).
    cache: HashMap<GlyphKey, Option<RasterizedGlyph>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    ch: char,
    bold: bool,
    italic: bool,
}

impl GlyphAtlas {
    pub fn new(font_family: &str, font_size: f32, line_height_factor: f32) -> Self {
        let mut font_system = FontSystem::new();

        // Measure cell dimensions using a reference character
        let metrics = Metrics::new(font_size, font_size * line_height_factor);
        let mut buffer = Buffer::new(&mut font_system, metrics);
        buffer.set_size(&mut font_system, Some(font_size * 4.0), Some(font_size * 2.0));

        let attrs = Attrs::new().family(cosmic_text::Family::Name(font_family));
        buffer.set_text(&mut font_system, "M", attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut font_system, false);

        // Approximate cell width from the 'M' character
        let cell_width = font_size * 0.6; // Monospace approximation
        let cell_height = font_size * line_height_factor;

        Self {
            font_system,
            swash_cache: SwashCache::new(),
            cell_width,
            cell_height,
            font_size,
            line_height: line_height_factor,
            cache: HashMap::new(),
        }
    }

    /// Get cell dimensions in pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.cell_height)
    }

    /// Update font metrics (e.g. after config reload).
    pub fn update_metrics(&mut self, font_size: f32, line_height_factor: f32) {
        self.font_size = font_size;
        self.line_height = line_height_factor;
        self.cell_width = font_size * 0.6;
        self.cell_height = font_size * line_height_factor;
        self.cache.clear();
    }
}
