use vte::{Perform, Params};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Black, Red, Green, Yellow, Blue, Magenta, Cyan, White,
    BrightBlack, BrightRed, BrightGreen, BrightYellow, BrightBlue, BrightMagenta, BrightCyan, BrightWhite,
    DefaultFg,
    DefaultBg,
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub char: char,
    pub fg: Color,
    pub bg: Color,
    pub inverse: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            char: ' ',
            fg: Color::DefaultFg,
            bg: Color::DefaultBg,
            inverse: false,
        }
    }
}

pub struct Terminal {
    pub grid: Vec<Vec<Cell>>,
    pub history: Vec<Vec<Cell>>,
    pub cols: usize,
    pub rows: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,
    pub scroll_offset: usize,

    // Scroll Region Margins (0-indexed, inclusive)
    pub scroll_top: usize,
    pub scroll_bottom: usize,

    pub current_fg: Color,
    pub current_bg: Color,
    pub current_inverse: bool,
    pub saved_cursor_x: usize,
    pub saved_cursor_y: usize,
    pub mouse_reporting: bool,

    pub title: String,

    // Selection Tracking
    pub selection_start: Option<(usize, usize)>,
    pub selection_end: Option<(usize, usize)>,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        let grid = vec![vec![Cell::default(); cols]; rows];
        Self {
            grid,
            history: Vec::new(),
            cols,
            rows,
            cursor_x: 0,
            cursor_y: 0,
            scroll_offset: 0,

            // Default scroll region is the full screen
            scroll_top: 0,
            scroll_bottom: rows.saturating_sub(1),

            current_fg: Color::DefaultFg,
            current_bg: Color::DefaultBg,
            current_inverse: false,
            saved_cursor_x: 0,
            saved_cursor_y: 0,
            mouse_reporting: false,
            title: "RoseTerm".to_string(),

            selection_start: None,
            selection_end: None,
        }
    }

    pub fn start_selection(&mut self, col: usize, row: usize) {
        self.selection_start = Some((col, row));
        self.selection_end = Some((col, row));
    }

    pub fn update_selection(&mut self, col: usize, row: usize) {
        if self.selection_start.is_some() {
            self.selection_end = Some((col, row));
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection_start = None;
        self.selection_end = None;
    }

    pub fn is_selected(&self, col: usize, row: usize) -> bool {
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let (p1, p2) = if start.1 < end.1 || (start.1 == end.1 && start.0 <= end.0) {
                (start, end)
            } else {
                (end, start)
            };

            if row < p1.1 || row > p2.1 { return false; }
            if row == p1.1 && row == p2.1 { return col >= p1.0 && col <= p2.0; }
            if row == p1.1 { return col >= p1.0; }
            if row == p2.1 { return col <= p2.0; }
            return true;
        }
        false
    }

    pub fn get_selected_text(&self) -> String {
        let mut text = String::new();
        if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
            let (p1, p2) = if start.1 < end.1 || (start.1 == end.1 && start.0 <= end.0) {
                (start, end)
            } else {
                (end, start)
            };

            for r in p1.1..=p2.1 {
                let row_data = self.get_visible_row(r);
                let start_col = if r == p1.1 { p1.0 } else { 0 };
                let end_col = if r == p2.1 { p2.0 } else { self.cols - 1 };

                for c in start_col..=end_col {
                    if c < row_data.len() {
                        text.push(row_data[c].char);
                    }
                }
                if r != p2.1 { text.push('\n'); }
            }
        }
        text
    }

    // FIX: Updated new_line to respect Scrolling Regions
    fn new_line(&mut self) {
        if self.cursor_y == self.scroll_bottom {
            // We are at the bottom of the scroll region.
            // Remove the top line of the region.
            let removed = self.grid.remove(self.scroll_top);

            // Only push to history if we are scrolling from the absolute top (0)
            if self.scroll_top == 0 {
                if self.history.len() > 10_000 {
                    self.history.remove(0);
                }
                self.history.push(removed);
            }

            // Insert a new blank line at the bottom of the region
            self.grid.insert(self.scroll_bottom, vec![self.blank_cell(); self.cols]);
        } else {
            // Otherwise, simply move the cursor down
            self.cursor_y += 1;
            // Safety clamp
            if self.cursor_y >= self.rows {
                self.cursor_y = self.rows - 1;
            }
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = (self.scroll_offset + lines).min(self.history.len());
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub fn get_visible_row(&self, screen_y: usize) -> &Vec<Cell> {
        if self.scroll_offset == 0 {
            &self.grid[screen_y]
        } else {
            let total_history = self.history.len();
            let rows_from_bottom = self.rows - 1 - screen_y;
            let effective_offset = self.scroll_offset + rows_from_bottom;

            if effective_offset >= self.rows {
                let history_index = total_history - (effective_offset - self.rows + 1);
                &self.history[history_index]
            } else {
                let grid_index = self.rows - effective_offset - 1;
                &self.grid[grid_index]
            }
        }
    }

    fn blank_cell(&self) -> Cell {
        Cell {
            char: ' ',
            fg: self.current_fg,
            bg: self.current_bg,
            inverse: self.current_inverse,
        }
    }

    pub fn resize(&mut self, new_cols: usize, new_rows: usize) {
        self.grid.resize(new_rows, vec![Cell::default(); new_cols]);
        for row in &mut self.grid {
            row.resize(new_cols, Cell::default());
        }
        self.rows = new_rows;
        self.cols = new_cols;
        // Reset scroll region to full screen on resize
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);

        self.cursor_x = self.cursor_x.min(self.cols - 1);
        self.cursor_y = self.cursor_y.min(self.rows - 1);
        self.scroll_offset = 0;
    }
}

impl Perform for Terminal {
    fn print(&mut self, c: char) {
        if self.cursor_x >= self.cols {
            self.new_line();
            self.cursor_x = 0;
        }
        self.grid[self.cursor_y][self.cursor_x] = Cell {
            char: c,
            fg: self.current_fg,
            bg: self.current_bg,
            inverse: self.current_inverse,
        };
        self.cursor_x += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\r' => self.cursor_x = 0,
            0x08 => { if self.cursor_x > 0 { self.cursor_x -= 1; } }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.len() >= 2 {
            let command = params[0];
            let text_bytes = params[1];
            if command == b"0" || command == b"2" {
                if let Ok(title_str) = std::str::from_utf8(text_bytes) {
                    self.title = title_str.to_string();
                }
            }
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let p = |i: usize| -> usize {
            let val = params.iter().nth(i).map(|x| x[0]).unwrap_or(1);
            if val == 0 { 1 } else { val as usize }
        };

        match action {
            'A' => self.cursor_y = self.cursor_y.saturating_sub(p(0)),
            'B' => self.cursor_y = (self.cursor_y + p(0)).min(self.rows - 1),
            'C' => self.cursor_x = (self.cursor_x + p(0)).min(self.cols - 1),
            'D' => self.cursor_x = self.cursor_x.saturating_sub(p(0)),
            'H' | 'f' => {
                let row = p(0).saturating_sub(1);
                let col = p(1).saturating_sub(1);
                self.cursor_y = row.min(self.rows - 1);
                self.cursor_x = col.min(self.cols - 1);
            }
            'G' => self.cursor_x = (p(0).saturating_sub(1)).min(self.cols - 1),
            'd' => self.cursor_y = (p(0).saturating_sub(1)).min(self.rows - 1),
            'J' => {
                let param = params.iter().next().map(|x| x[0]).unwrap_or(0);
                let clear_cell = |c: &mut Cell| {
                    c.char = ' ';
                    c.fg = Color::DefaultFg;
                    c.bg = Color::DefaultBg;
                    c.inverse = false;
                };
                match param {
                    2 => { for row in &mut self.grid { for cell in row { clear_cell(cell); } } self.cursor_x = 0; self.cursor_y = 0; },
                    0 | _ => {
                        if self.cursor_y < self.rows { for x in self.cursor_x..self.cols { clear_cell(&mut self.grid[self.cursor_y][x]); } }
                        for y in (self.cursor_y + 1)..self.rows { for cell in &mut self.grid[y] { clear_cell(cell); } }
                    }
                }
            }
            'K' => {
                let param = params.iter().next().map(|x| x[0]).unwrap_or(0);
                let clear_cell = |c: &mut Cell| {
                    c.char = ' ';
                    c.fg = Color::DefaultFg;
                    c.bg = Color::DefaultBg;
                    c.inverse = false;
                };
                match param {
                    2 => { for cell in &mut self.grid[self.cursor_y] { clear_cell(cell); } },
                    1 => { for x in 0..self.cursor_x { clear_cell(&mut self.grid[self.cursor_y][x]); } },
                    0 | _ => { for x in self.cursor_x..self.cols { clear_cell(&mut self.grid[self.cursor_y][x]); } }
                }
            }
            // FIX: Updated L (Insert Line) to respect margins
            'L' => {
                let count = p(0);
                let cy = self.cursor_y;
                let blank_row = vec![self.blank_cell(); self.cols];

                // Only insert if cursor is inside the scroll region
                if cy >= self.scroll_top && cy <= self.scroll_bottom {
                    for _ in 0..count {
                        self.grid.remove(self.scroll_bottom);
                        self.grid.insert(cy, blank_row.clone());
                    }
                }
            }
            // FIX: Updated M (Delete Line) to respect margins
            'M' => {
                let count = p(0);
                let cy = self.cursor_y;
                let blank_row = vec![self.blank_cell(); self.cols];

                // Only delete if cursor is inside the scroll region
                if cy >= self.scroll_top && cy <= self.scroll_bottom {
                    for _ in 0..count {
                        self.grid.remove(cy);
                        self.grid.insert(self.scroll_bottom, blank_row.clone());
                    }
                }
            }
            'P' => {
                let count = p(0);
                let cx = self.cursor_x;
                let cy = self.cursor_y;
                let blank = self.blank_cell();
                for _ in 0..count {
                    if cx < self.grid[cy].len() {
                        self.grid[cy].remove(cx);
                        self.grid[cy].push(blank);
                    }
                }
            }
            '@' => {
                let count = p(0);
                let cx = self.cursor_x;
                let cy = self.cursor_y;
                let blank = self.blank_cell();
                for _ in 0..count {
                    if cx < self.cols {
                        self.grid[cy].insert(cx, blank);
                        self.grid[cy].pop();
                    }
                }
            }
            // FIX: Added 'r' (DECSTBM - Set Top and Bottom Margins)
            'r' => {
                let top = p(0).saturating_sub(1);
                // If param 1 is missing, it usually defaults to bottom of screen
                let bot = if params.len() > 1 { p(1).saturating_sub(1) } else { self.rows - 1 };

                self.scroll_top = top.min(self.rows - 1);
                self.scroll_bottom = bot.min(self.rows - 1);

                // Validation: Bottom must be > Top
                if self.scroll_bottom <= self.scroll_top {
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows.saturating_sub(1);
                }

                // CSI r always moves cursor to (0,0) according to spec
                self.cursor_x = 0;
                self.cursor_y = 0;
            }
            'h' => {
                 for p in params {
                     match p[0] {
                         1000 | 1002 | 1006 | 1015 => self.mouse_reporting = true,
                         25 => { }
                         _ => {}
                     }
                 }
            }
            'l' => {
                 for p in params {
                     match p[0] {
                         1000 | 1002 | 1006 | 1015 => self.mouse_reporting = false,
                         25 => { }
                         _ => {}
                     }
                 }
            }
            'm' => {
                if params.len() == 0 {
                    self.current_fg = Color::DefaultFg;
                    self.current_bg = Color::DefaultBg;
                    self.current_inverse = false;
                    return;
                }
                for p_iter in params {
                    match p_iter[0] {
                        0 => { self.current_fg = Color::DefaultFg; self.current_bg = Color::DefaultBg; self.current_inverse = false; }
                        1 => {
                            self.current_fg = match self.current_fg {
                                Color::Black => Color::BrightBlack,
                                Color::Red => Color::BrightRed,
                                Color::Green => Color::BrightGreen,
                                Color::Yellow => Color::BrightYellow,
                                Color::Blue => Color::BrightBlue,
                                Color::Magenta => Color::BrightMagenta,
                                Color::Cyan => Color::BrightCyan,
                                Color::White => Color::BrightWhite,
                                _ => self.current_fg,
                            };
                        }
                        7 => self.current_inverse = true,
                        27 => self.current_inverse = false,
                        30 => self.current_fg = Color::Black,
                        31 => self.current_fg = Color::Red,
                        32 => self.current_fg = Color::Green,
                        33 => self.current_fg = Color::Yellow,
                        34 => self.current_fg = Color::Blue,
                        35 => self.current_fg = Color::Magenta,
                        36 => self.current_fg = Color::Cyan,
                        37 => self.current_fg = Color::White,
                        39 => self.current_fg = Color::DefaultFg,
                        40 => self.current_bg = Color::Black,
                        41 => self.current_bg = Color::Red,
                        42 => self.current_bg = Color::Green,
                        43 => self.current_bg = Color::Yellow,
                        44 => self.current_bg = Color::Blue,
                        45 => self.current_bg = Color::Magenta,
                        46 => self.current_bg = Color::Cyan,
                        47 => self.current_bg = Color::White,
                        49 => self.current_bg = Color::DefaultBg,
                        90 => self.current_fg = Color::BrightBlack,
                        91 => self.current_fg = Color::BrightRed,
                        92 => self.current_fg = Color::BrightGreen,
                        93 => self.current_fg = Color::BrightYellow,
                        94 => self.current_fg = Color::BrightBlue,
                        95 => self.current_fg = Color::BrightMagenta,
                        96 => self.current_fg = Color::BrightCyan,
                        97 => self.current_fg = Color::BrightWhite,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}