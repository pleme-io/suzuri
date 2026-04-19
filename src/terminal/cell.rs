/// A single cell in the terminal grid.
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: CellAttrs,
}

/// Color representation for terminal cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Color {
    /// Default foreground/background from config.
    Default,
    /// ANSI 256-color index.
    Indexed(u8),
    /// True color RGB.
    Rgb(u8, u8, u8),
}

/// Text attributes for a cell.
#[derive(Debug, Clone, Copy, Default)]
pub struct CellAttrs {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub dim: bool,
    pub inverse: bool,
    pub hidden: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::Default,
            bg: Color::Default,
            attrs: CellAttrs::default(),
        }
    }
}

impl Cell {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
