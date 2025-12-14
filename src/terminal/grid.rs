use vte::{Perform, Params};

// 1. Define the Colors available
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Black, Red, Green, Yellow, Blue, Magenta, Cyan, White,
    BrightBlack, BrightRed, BrightGreen, BrightYellow, BrightBlue, BrightMagenta, BrightCyan, BrightWhite,
    DefaultFg,
    DefaultBg,
}

// 2. Add Color info to the Cell
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub char: char,
    pub fg: Color,
    pub bg: Color,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            char: ' ',
            fg: Color::DefaultFg,
            bg: Color::DefaultBg
        }
    }
}

pub struct Terminal {
    pub grid: Vec<Vec<Cell>>,
    pub cols: usize,
    pub rows: usize,
    pub cursor_x: usize,
    pub cursor_y: usize,

    // 3. The "Pen" stores the *current* color settings.
    pub current_fg: Color,
    pub current_bg: Color,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        let grid = vec![vec![Cell::default(); cols]; rows];
        Self {
            grid,
            cols,
            rows,
            cursor_x: 0,
            cursor_y: 0,
            current_fg: Color::DefaultFg,
            current_bg: Color::DefaultBg,
        }
    }

    fn new_line(&mut self) {
        self.cursor_y += 1;
        if self.cursor_y >= self.rows {
            self.cursor_y = self.rows - 1;
            self.grid.remove(0);
            // New lines get the default background
            self.grid.push(vec![Cell::default(); self.cols]);
        }
        self.cursor_x = 0;
    }
}

impl Perform for Terminal {
    fn print(&mut self, c: char) {
        if self.cursor_x >= self.cols {
            self.new_line();
        }

        // Apply the current Pen color to the cell
        self.grid[self.cursor_y][self.cursor_x] = Cell {
            char: c,
            fg: self.current_fg,
            bg: self.current_bg,
        };
        self.cursor_x += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\r' => self.cursor_x = 0,
            0x08 => { // Backspace
                if self.cursor_x > 0 {
                    self.cursor_x -= 1;
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let param = params.iter().next().map(|p| p[0]).unwrap_or(0);

        match action {
            'A' => { // Up
                 let amount = std::cmp::max(1, param) as usize;
                 self.cursor_y = self.cursor_y.saturating_sub(amount);
            }
            'B' => { // Down
                 let amount = std::cmp::max(1, param) as usize;
                 self.cursor_y = (self.cursor_y + amount).min(self.rows - 1);
            }
            'C' => { // Right
                 let amount = std::cmp::max(1, param) as usize;
                 self.cursor_x = (self.cursor_x + amount).min(self.cols - 1);
            }
            'D' => { // Left
                 let amount = std::cmp::max(1, param) as usize;
                 self.cursor_x = self.cursor_x.saturating_sub(amount);
            }
            'J' => { // Erase Screen
                // Helper to clear a cell but keep default colors
                let clear_cell = |c: &mut Cell| {
                    c.char = ' ';
                    c.fg = Color::DefaultFg; // Fixed: using = instead of :
                    c.bg = Color::DefaultBg; // Fixed: using = instead of :
                };

                match param {
                    2 => {
                        for row in &mut self.grid {
                            for cell in row { clear_cell(cell); }
                        }
                        self.cursor_x = 0; self.cursor_y = 0;
                    },
                    0 | _ => {
                        if self.cursor_y < self.rows {
                             for x in self.cursor_x..self.cols {
                                 clear_cell(&mut self.grid[self.cursor_y][x]);
                             }
                        }
                        for y in (self.cursor_y + 1)..self.rows {
                            for cell in &mut self.grid[y] { clear_cell(cell); }
                        }
                    }
                }
            }
            'K' => { // Erase Line
                 let clear_cell = |c: &mut Cell| {
                    c.char = ' ';
                    c.fg = Color::DefaultFg; // Fixed
                    c.bg = Color::DefaultBg; // Fixed
                };
                match param {
                    2 => { for cell in &mut self.grid[self.cursor_y] { clear_cell(cell); } },
                    1 => { for x in 0..self.cursor_x { clear_cell(&mut self.grid[self.cursor_y][x]); } },
                    0 | _ => { for x in self.cursor_x..self.cols { clear_cell(&mut self.grid[self.cursor_y][x]); } }
                }
            }
            // 4. Handle SGR (Select Graphic Rendition) - Colors!
            'm' => {
                if params.len() == 0 {
                    self.current_fg = Color::DefaultFg;
                    self.current_bg = Color::DefaultBg;
                    return;
                }

                for p in params {
                    let code = p[0];
                    match code {
                        0 => {
                            self.current_fg = Color::DefaultFg;
                            self.current_bg = Color::DefaultBg;
                        }
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