use anyhow::Result;
use fontdue::{Font, FontSettings};
use crate::terminal::grid::{Terminal, Color};

pub struct FontRenderer {
    font: Font,
    char_width: f32,
    char_height: f32,
}

impl FontRenderer {
    pub fn new() -> Result<Self> {
        // Use your preferred font path here
        let font_data = std::fs::read("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf")
            .or_else(|_| std::fs::read("/usr/share/fonts/liberation/LiberationMono-Regular.ttf"))
            .or_else(|_| std::fs::read("/usr/share/fonts/gnu-free/FreeMono.ttf"))
            .expect("Could not find a font file!");

        let font = Font::from_bytes(font_data, FontSettings::default())
            .map_err(|e| anyhow::anyhow!("Error loading font: {}", e))?;

        let metrics = font.metrics('M', 18.0);

        Ok(Self {
            font,
            char_width: metrics.advance_width,
            char_height: 22.0,
        })
    }

    // Helper to convert our Color enum to RGB bytes
    fn color_to_rgb(&self, color: Color) -> (u8, u8, u8) {
        match color {
            Color::Black => (0, 0, 0),
            Color::Red => (205, 49, 49),
            Color::Green => (13, 188, 121),
            Color::Yellow => (229, 229, 16),
            Color::Blue => (36, 114, 200),
            Color::Magenta => (188, 63, 188),
            Color::Cyan => (17, 168, 205),
            Color::White => (229, 229, 229),

            Color::BrightBlack => (102, 102, 102),
            Color::BrightRed => (241, 76, 76),
            Color::BrightGreen => (35, 209, 139),
            Color::BrightYellow => (245, 245, 67),
            Color::BrightBlue => (59, 142, 234),
            Color::BrightMagenta => (214, 112, 214),
            Color::BrightCyan => (41, 184, 219),
            Color::BrightWhite => (255, 255, 255),

            Color::DefaultFg => (229, 229, 229), // Default Text is White-ish
            Color::DefaultBg => (16, 16, 24),    // Default BG is Dark
        }
    }

    pub fn draw(&self, term: &Terminal, frame: &mut [u8], screen_width: u32) {
        // 1. Clear screen to Default BG color
        let (bg_r, bg_g, bg_b) = self.color_to_rgb(Color::DefaultBg);
        for pixel in frame.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[bg_r, bg_g, bg_b, 255]);
        }

        for (row_idx, row) in term.grid.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                // Handle Background Color (if it's not the default)
                if cell.bg != Color::DefaultBg {
                     let (br, bg, bb) = self.color_to_rgb(cell.bg);
                     let cx = (col_idx as f32 * self.char_width) as usize;
                     let cy = (row_idx as f32 * self.char_height) as usize;
                     let cw = self.char_width.ceil() as usize;
                     let ch = self.char_height.ceil() as usize;

                     // Draw a rectangle for the background
                     for y in cy..(cy+ch) {
                         for x in cx..(cx+cw) {
                             if x >= screen_width as usize { continue; }
                             let idx = (y * screen_width as usize + x) * 4;
                             if idx + 3 < frame.len() {
                                 frame[idx] = br;
                                 frame[idx+1] = bg;
                                 frame[idx+2] = bb;
                                 frame[idx+3] = 255;
                             }
                         }
                     }
                }

                if cell.char == '\0' || cell.char == ' ' { continue; }

                let (metrics, bitmap) = self.font.rasterize(cell.char, 18.0);
                if metrics.width == 0 || metrics.height == 0 { continue; }

                let cell_x_start = (col_idx as f32 * self.char_width) as i32;
                let cell_y_start = (row_idx as f32 * self.char_height) as i32;
                let baseline_y = cell_y_start + 16;

                // Get Foreground Color
                let (fg_r, fg_g, fg_b) = self.color_to_rgb(cell.fg);

                for (i, coverage) in bitmap.into_iter().enumerate() {
                    let x_in_bitmap = (i % metrics.width) as i32;
                    let y_in_bitmap = (i / metrics.width) as i32;
                    let y_offset_from_baseline = -(metrics.ymin + metrics.height as i32) + y_in_bitmap;

                    let x = cell_x_start + x_in_bitmap + metrics.xmin;
                    let y = baseline_y + y_offset_from_baseline;

                    if x < 0 || x >= screen_width as i32 || y < 0 { continue; }

                    let idx = (y as usize * screen_width as usize + x as usize) * 4;

                    if idx + 3 < frame.len() {
                        // Blend text color
                        let alpha = coverage as f32 / 255.0;
                        let inv_alpha = 1.0 - alpha;

                        // Simple blending with whatever is behind it (background color)
                        let current_r = frame[idx] as f32;
                        let current_g = frame[idx+1] as f32;
                        let current_b = frame[idx+2] as f32;

                        frame[idx] = (fg_r as f32 * alpha + current_r * inv_alpha) as u8;
                        frame[idx+1] = (fg_g as f32 * alpha + current_g * inv_alpha) as u8;
                        frame[idx+2] = (fg_b as f32 * alpha + current_b * inv_alpha) as u8;
                        frame[idx+3] = 255;
                    }
                }
            }
        }

        // Draw Cursor
        let cx = (term.cursor_x as f32 * self.char_width) as usize;
        let cy = (term.cursor_y as f32 * self.char_height) as usize;
        let cursor_h = self.char_height as usize;
        let cursor_w = self.char_width as usize;

        for y in cy..(cy + cursor_h) {
            for x in cx..(cx + cursor_w) {
                let idx = (y * screen_width as usize + x) * 4;
                if idx + 3 < frame.len() {
                    // Invert color for cursor effect
                    frame[idx] = 255 - frame[idx];
                    frame[idx+1] = 255 - frame[idx+1];
                    frame[idx+2] = 255 - frame[idx+2];
                    frame[idx+3] = 255;
                }
            }
        }
    }
}