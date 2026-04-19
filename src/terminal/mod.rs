pub mod cell;

use cell::{Cell, CellAttrs, Color};

/// Terminal grid with scrollback buffer.
pub struct Terminal {
    /// Visible grid: rows × cols.
    grid: Vec<Vec<Cell>>,
    /// Scrollback buffer (oldest first).
    scrollback: Vec<Vec<Cell>>,
    /// Maximum scrollback lines.
    max_scrollback: usize,
    /// Grid dimensions.
    pub cols: usize,
    pub rows: usize,
    /// Cursor position (0-indexed).
    pub cursor_row: usize,
    pub cursor_col: usize,
    /// Current text attributes for new characters.
    current_attrs: CellAttrs,
    current_fg: Color,
    current_bg: Color,
    /// Scroll region (top, bottom) — inclusive, 0-indexed.
    scroll_top: usize,
    scroll_bottom: usize,
    /// Whether content has changed since last render.
    pub dirty: bool,
    /// Saved cursor position (for ESC 7 / ESC 8).
    saved_cursor: Option<(usize, usize)>,
    /// Alternate screen buffer.
    alt_grid: Option<Vec<Vec<Cell>>>,
    alt_cursor: Option<(usize, usize)>,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, max_scrollback: usize) -> Self {
        let grid = vec![vec![Cell::default(); cols]; rows];
        Self {
            grid,
            scrollback: Vec::new(),
            max_scrollback,
            cols,
            rows,
            cursor_row: 0,
            cursor_col: 0,
            current_attrs: CellAttrs::default(),
            current_fg: Color::Default,
            current_bg: Color::Default,
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),
            dirty: true,
            saved_cursor: None,
            alt_grid: None,
            alt_cursor: None,
        }
    }

    /// Access a cell at (row, col).
    pub fn cell(&self, row: usize, col: usize) -> &Cell {
        &self.grid[row][col]
    }

    /// Iterate over all visible rows.
    pub fn rows_iter(&self) -> impl Iterator<Item = &[Cell]> {
        self.grid.iter().map(|row| row.as_slice())
    }

    /// Resize the terminal grid, preserving content where possible.
    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        let mut new_grid = vec![vec![Cell::default(); new_cols]; new_rows];
        let copy_rows = new_rows.min(self.rows);
        let copy_cols = new_cols.min(self.cols);
        for r in 0..copy_rows {
            for c in 0..copy_cols {
                new_grid[r][c] = self.grid[r][c];
            }
        }
        self.grid = new_grid;
        self.cols = new_cols;
        self.rows = new_rows;
        self.cursor_row = self.cursor_row.min(new_rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(new_cols.saturating_sub(1));
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);
        self.dirty = true;
    }

    /// Write a character at the cursor position and advance.
    fn put_char(&mut self, ch: char) {
        if self.cursor_col >= self.cols {
            // Auto-wrap
            self.cursor_col = 0;
            self.linefeed();
        }
        if self.cursor_row < self.rows && self.cursor_col < self.cols {
            let cell = &mut self.grid[self.cursor_row][self.cursor_col];
            cell.ch = ch;
            cell.fg = self.current_fg;
            cell.bg = self.current_bg;
            cell.attrs = self.current_attrs;
            self.cursor_col += 1;
        }
        self.dirty = true;
    }

    /// Line feed: move cursor down, scroll if at bottom of scroll region.
    fn linefeed(&mut self) {
        if self.cursor_row == self.scroll_bottom {
            self.scroll_up(1);
        } else if self.cursor_row < self.rows - 1 {
            self.cursor_row += 1;
        }
    }

    /// Scroll the scroll region up by n lines.
    fn scroll_up(&mut self, n: usize) {
        for _ in 0..n {
            // Push top line of scroll region to scrollback
            if self.scroll_top == 0 {
                let line = self.grid[0].clone();
                self.scrollback.push(line);
                if self.scrollback.len() > self.max_scrollback {
                    self.scrollback.remove(0);
                }
            }
            // Shift lines up within scroll region
            for r in self.scroll_top..self.scroll_bottom {
                self.grid[r] = self.grid[r + 1].clone();
            }
            // Clear bottom line of scroll region
            self.grid[self.scroll_bottom] = vec![Cell::default(); self.cols];
        }
        self.dirty = true;
    }

    /// Scroll the scroll region down by n lines.
    fn scroll_down(&mut self, n: usize) {
        for _ in 0..n {
            for r in (self.scroll_top + 1..=self.scroll_bottom).rev() {
                self.grid[r] = self.grid[r - 1].clone();
            }
            self.grid[self.scroll_top] = vec![Cell::default(); self.cols];
        }
        self.dirty = true;
    }

    /// Erase from cursor to end of line.
    fn erase_to_eol(&mut self) {
        for c in self.cursor_col..self.cols {
            self.grid[self.cursor_row][c].reset();
        }
        self.dirty = true;
    }

    /// Erase from start of line to cursor.
    fn erase_to_bol(&mut self) {
        for c in 0..=self.cursor_col.min(self.cols - 1) {
            self.grid[self.cursor_row][c].reset();
        }
        self.dirty = true;
    }

    /// Erase entire line.
    fn erase_line(&mut self) {
        for c in 0..self.cols {
            self.grid[self.cursor_row][c].reset();
        }
        self.dirty = true;
    }

    /// Erase from cursor to end of screen.
    fn erase_below(&mut self) {
        self.erase_to_eol();
        for r in (self.cursor_row + 1)..self.rows {
            for c in 0..self.cols {
                self.grid[r][c].reset();
            }
        }
        self.dirty = true;
    }

    /// Erase from start of screen to cursor.
    fn erase_above(&mut self) {
        self.erase_to_bol();
        for r in 0..self.cursor_row {
            for c in 0..self.cols {
                self.grid[r][c].reset();
            }
        }
        self.dirty = true;
    }

    /// Erase entire screen.
    fn erase_screen(&mut self) {
        for r in 0..self.rows {
            for c in 0..self.cols {
                self.grid[r][c].reset();
            }
        }
        self.dirty = true;
    }

    /// Enter alternate screen buffer.
    fn enter_alt_screen(&mut self) {
        if self.alt_grid.is_none() {
            self.alt_grid = Some(self.grid.clone());
            self.alt_cursor = Some((self.cursor_row, self.cursor_col));
            self.erase_screen();
            self.cursor_row = 0;
            self.cursor_col = 0;
        }
    }

    /// Leave alternate screen buffer.
    fn leave_alt_screen(&mut self) {
        if let Some(grid) = self.alt_grid.take() {
            self.grid = grid;
            if let Some((row, col)) = self.alt_cursor.take() {
                self.cursor_row = row;
                self.cursor_col = col;
            }
            self.dirty = true;
        }
    }

    /// Apply SGR (Select Graphic Rendition) parameter.
    fn apply_sgr(&mut self, param: u16) {
        match param {
            0 => {
                self.current_attrs = CellAttrs::default();
                self.current_fg = Color::Default;
                self.current_bg = Color::Default;
            }
            1 => self.current_attrs.bold = true,
            2 => self.current_attrs.dim = true,
            3 => self.current_attrs.italic = true,
            4 => self.current_attrs.underline = true,
            7 => self.current_attrs.inverse = true,
            8 => self.current_attrs.hidden = true,
            9 => self.current_attrs.strikethrough = true,
            22 => {
                self.current_attrs.bold = false;
                self.current_attrs.dim = false;
            }
            23 => self.current_attrs.italic = false,
            24 => self.current_attrs.underline = false,
            27 => self.current_attrs.inverse = false,
            28 => self.current_attrs.hidden = false,
            29 => self.current_attrs.strikethrough = false,
            30..=37 => self.current_fg = Color::Indexed((param - 30) as u8),
            39 => self.current_fg = Color::Default,
            40..=47 => self.current_bg = Color::Indexed((param - 40) as u8),
            49 => self.current_bg = Color::Default,
            90..=97 => self.current_fg = Color::Indexed((param - 90 + 8) as u8),
            100..=107 => self.current_bg = Color::Indexed((param - 100 + 8) as u8),
            _ => {}
        }
    }
}

// ── VTE Perform Implementation ────────────────────────────────

impl vte::Perform for Terminal {
    fn print(&mut self, c: char) {
        self.put_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            // BEL
            0x07 => {}
            // BS (backspace)
            0x08 => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            // HT (horizontal tab)
            0x09 => {
                let next_tab = (self.cursor_col / 8 + 1) * 8;
                self.cursor_col = next_tab.min(self.cols - 1);
            }
            // LF, VT, FF (line feed variants)
            0x0A | 0x0B | 0x0C => {
                self.linefeed();
            }
            // CR (carriage return)
            0x0D => {
                self.cursor_col = 0;
            }
            _ => {}
        }
        self.dirty = true;
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates, byte) {
            // ESC 7 — save cursor
            ([], b'7') => {
                self.saved_cursor = Some((self.cursor_row, self.cursor_col));
            }
            // ESC 8 — restore cursor
            ([], b'8') => {
                if let Some((row, col)) = self.saved_cursor {
                    self.cursor_row = row.min(self.rows - 1);
                    self.cursor_col = col.min(self.cols - 1);
                }
            }
            // ESC D — index (move down, scroll if needed)
            ([], b'D') => {
                self.linefeed();
            }
            // ESC M — reverse index (move up, scroll if needed)
            ([], b'M') => {
                if self.cursor_row == self.scroll_top {
                    self.scroll_down(1);
                } else if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                }
            }
            _ => {}
        }
        self.dirty = true;
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let mut params_iter = params.iter();
        let first = params_iter
            .next()
            .and_then(|p| p.first().copied())
            .unwrap_or(0);
        let second = params_iter
            .next()
            .and_then(|p| p.first().copied())
            .unwrap_or(0);

        match (action, intermediates) {
            // CUU — cursor up
            ('A', []) => {
                let n = first.max(1) as usize;
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            // CUD — cursor down
            ('B', []) => {
                let n = first.max(1) as usize;
                self.cursor_row = (self.cursor_row + n).min(self.rows - 1);
            }
            // CUF — cursor forward
            ('C', []) => {
                let n = first.max(1) as usize;
                self.cursor_col = (self.cursor_col + n).min(self.cols - 1);
            }
            // CUB — cursor back
            ('D', []) => {
                let n = first.max(1) as usize;
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            // CUP — cursor position
            ('H', []) | ('f', []) => {
                let row = (first.max(1) as usize).saturating_sub(1);
                let col = (second.max(1) as usize).saturating_sub(1);
                self.cursor_row = row.min(self.rows - 1);
                self.cursor_col = col.min(self.cols - 1);
            }
            // ED — erase in display
            ('J', []) => match first {
                0 => self.erase_below(),
                1 => self.erase_above(),
                2 | 3 => self.erase_screen(),
                _ => {}
            },
            // EL — erase in line
            ('K', []) => match first {
                0 => self.erase_to_eol(),
                1 => self.erase_to_bol(),
                2 => self.erase_line(),
                _ => {}
            },
            // IL — insert lines
            ('L', []) => {
                let n = first.max(1) as usize;
                for _ in 0..n {
                    if self.cursor_row <= self.scroll_bottom {
                        for r in (self.cursor_row + 1..=self.scroll_bottom).rev() {
                            self.grid[r] = self.grid[r - 1].clone();
                        }
                        self.grid[self.cursor_row] = vec![Cell::default(); self.cols];
                    }
                }
                self.dirty = true;
            }
            // DL — delete lines
            ('M', []) => {
                let n = first.max(1) as usize;
                for _ in 0..n {
                    if self.cursor_row <= self.scroll_bottom {
                        for r in self.cursor_row..self.scroll_bottom {
                            self.grid[r] = self.grid[r + 1].clone();
                        }
                        self.grid[self.scroll_bottom] = vec![Cell::default(); self.cols];
                    }
                }
                self.dirty = true;
            }
            // DCH — delete characters
            ('P', []) => {
                let n = (first.max(1) as usize).min(self.cols - self.cursor_col);
                let row = &mut self.grid[self.cursor_row];
                for c in self.cursor_col..(self.cols - n) {
                    row[c] = row[c + n];
                }
                for c in (self.cols - n)..self.cols {
                    row[c].reset();
                }
                self.dirty = true;
            }
            // SU — scroll up
            ('S', []) => {
                let n = first.max(1) as usize;
                self.scroll_up(n);
            }
            // SD — scroll down
            ('T', []) => {
                let n = first.max(1) as usize;
                self.scroll_down(n);
            }
            // ICH — insert characters
            ('@', []) => {
                let n = (first.max(1) as usize).min(self.cols - self.cursor_col);
                let row = &mut self.grid[self.cursor_row];
                for c in (self.cursor_col + n..self.cols).rev() {
                    row[c] = row[c - n];
                }
                for c in self.cursor_col..(self.cursor_col + n) {
                    row[c].reset();
                }
                self.dirty = true;
            }
            // SGR — select graphic rendition
            ('m', []) => {
                let mut iter = params.iter();
                if params.is_empty() {
                    self.apply_sgr(0);
                    return;
                }
                while let Some(param) = iter.next() {
                    let p = param.first().copied().unwrap_or(0);
                    match p {
                        // Extended foreground color
                        38 => {
                            if let Some(next) = iter.next() {
                                match next.first().copied().unwrap_or(0) {
                                    5 => {
                                        if let Some(idx) = iter.next() {
                                            self.current_fg =
                                                Color::Indexed(idx[0] as u8);
                                        }
                                    }
                                    2 => {
                                        let r = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                        let g = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                        let b = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                        self.current_fg = Color::Rgb(r, g, b);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        // Extended background color
                        48 => {
                            if let Some(next) = iter.next() {
                                match next.first().copied().unwrap_or(0) {
                                    5 => {
                                        if let Some(idx) = iter.next() {
                                            self.current_bg =
                                                Color::Indexed(idx[0] as u8);
                                        }
                                    }
                                    2 => {
                                        let r = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                        let g = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                        let b = iter.next().map(|p| p[0] as u8).unwrap_or(0);
                                        self.current_bg = Color::Rgb(r, g, b);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => self.apply_sgr(p),
                    }
                }
            }
            // DECSTBM — set scroll region
            ('r', []) => {
                let top = (first.max(1) as usize).saturating_sub(1);
                let bottom = if second == 0 {
                    self.rows - 1
                } else {
                    (second as usize).saturating_sub(1).min(self.rows - 1)
                };
                if top < bottom {
                    self.scroll_top = top;
                    self.scroll_bottom = bottom;
                    self.cursor_row = 0;
                    self.cursor_col = 0;
                }
            }
            // DECSET / DECRST — private mode set/reset
            ('h', [b'?']) | ('l', [b'?']) => {
                let set = action == 'h';
                match first {
                    // Alternate screen buffer
                    1049 => {
                        if set {
                            self.enter_alt_screen();
                        } else {
                            self.leave_alt_screen();
                        }
                    }
                    // Cursor visibility (handled by renderer)
                    25 => {}
                    _ => {}
                }
            }
            // DSR — device status report (cursor position)
            ('n', []) if first == 6 => {
                // This would need to write back to PTY — handled externally
            }
            _ => {}
        }
        self.dirty = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_terminal_has_correct_dimensions() {
        let term = Terminal::new(80, 24, 1000);
        assert_eq!(term.cols, 80);
        assert_eq!(term.rows, 24);
        assert_eq!(term.cursor_row, 0);
        assert_eq!(term.cursor_col, 0);
    }

    #[test]
    fn put_char_advances_cursor() {
        let mut term = Terminal::new(80, 24, 1000);
        term.put_char('A');
        assert_eq!(term.cursor_col, 1);
        assert_eq!(term.cell(0, 0).ch, 'A');
    }

    #[test]
    fn linefeed_scrolls_at_bottom() {
        let mut term = Terminal::new(80, 3, 1000);
        term.put_char('A');
        term.cursor_row = 2;
        term.cursor_col = 0;
        term.put_char('B');
        term.linefeed();
        // Should have scrolled: row 0 is now what was row 1
        assert_eq!(term.cursor_row, 2);
        assert_eq!(term.scrollback.len(), 1);
    }

    #[test]
    fn erase_screen_clears_all_cells() {
        let mut term = Terminal::new(80, 24, 1000);
        term.put_char('X');
        term.erase_screen();
        assert_eq!(term.cell(0, 0).ch, ' ');
    }

    #[test]
    fn resize_preserves_content() {
        let mut term = Terminal::new(80, 24, 1000);
        term.put_char('A');
        term.resize(40, 12);
        assert_eq!(term.cols, 40);
        assert_eq!(term.rows, 12);
        assert_eq!(term.cell(0, 0).ch, 'A');
    }

    #[test]
    fn sgr_sets_colors() {
        let mut term = Terminal::new(80, 24, 1000);
        term.apply_sgr(31); // red foreground
        assert_eq!(term.current_fg, Color::Indexed(1));
        term.apply_sgr(0); // reset
        assert_eq!(term.current_fg, Color::Default);
    }

    #[test]
    fn autowrap_at_line_end() {
        let mut term = Terminal::new(3, 2, 1000);
        term.put_char('A');
        term.put_char('B');
        term.put_char('C');
        // Next char should wrap to next line
        term.put_char('D');
        assert_eq!(term.cursor_row, 1);
        assert_eq!(term.cursor_col, 1);
        assert_eq!(term.cell(1, 0).ch, 'D');
    }

    #[test]
    fn alt_screen_saves_and_restores() {
        let mut term = Terminal::new(80, 24, 1000);
        term.put_char('A');
        term.enter_alt_screen();
        assert_eq!(term.cell(0, 0).ch, ' ');
        term.put_char('B');
        term.leave_alt_screen();
        assert_eq!(term.cell(0, 0).ch, 'A');
    }

    #[test]
    fn vte_print_writes_characters() {
        use vte::Perform;
        let mut term = Terminal::new(80, 24, 1000);
        term.print('H');
        term.print('i');
        assert_eq!(term.cell(0, 0).ch, 'H');
        assert_eq!(term.cell(0, 1).ch, 'i');
        assert_eq!(term.cursor_col, 2);
    }
}
