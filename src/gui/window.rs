use anyhow::Result;
use log::error;
use pixels::{Pixels, SurfaceTexture};
use winit::{
    dpi::LogicalSize,
    event::{Event, VirtualKeyCode},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use winit_input_helper::{WinitInputHelper, TextChar};
use vte::Parser;
use arboard::Clipboard;
use std::time::{Duration, Instant};
use std::io::Write; // Explicitly import Write for the pty writer

use crate::backend::pty::Pty;
use crate::terminal::grid::Terminal;
use crate::renderer::font::FontRenderer;

#[derive(Debug)]
pub enum RoseEvent {
    PtyOutput(Vec<u8>),
    Exit,
}

fn encode_mouse(button: u8, x: usize, y: usize, release: bool) -> String {
    let suffix = if release { 'm' } else { 'M' };
    format!("\x1b[<{};{};{}{}", button, x + 1, y + 1, suffix)
}

// Helper to map A-Z to Control Codes (1-26)
fn ctrl_key_to_byte(key: VirtualKeyCode) -> Option<u8> {
    match key {
        VirtualKeyCode::A => Some(1),
        VirtualKeyCode::B => Some(2),
        VirtualKeyCode::C => Some(3),
        VirtualKeyCode::D => Some(4),
        VirtualKeyCode::E => Some(5),
        VirtualKeyCode::F => Some(6),
        VirtualKeyCode::G => Some(7),
        VirtualKeyCode::H => Some(8),
        VirtualKeyCode::I => Some(9),
        VirtualKeyCode::J => Some(10),
        VirtualKeyCode::K => Some(11),
        VirtualKeyCode::L => Some(12),
        VirtualKeyCode::M => Some(13),
        VirtualKeyCode::N => Some(14),
        VirtualKeyCode::O => Some(15),
        VirtualKeyCode::P => Some(16),
        VirtualKeyCode::Q => Some(17),
        VirtualKeyCode::R => Some(18),
        VirtualKeyCode::S => Some(19),
        VirtualKeyCode::T => Some(20),
        VirtualKeyCode::U => Some(21),
        VirtualKeyCode::V => Some(22),
        VirtualKeyCode::W => Some(23),
        VirtualKeyCode::X => Some(24),
        VirtualKeyCode::Y => Some(25),
        VirtualKeyCode::Z => Some(26),
        // Bracket/Symbol control codes often used in terminals
        VirtualKeyCode::LBracket => Some(27), // Esc
        VirtualKeyCode::Backslash => Some(28),
        VirtualKeyCode::RBracket => Some(29),
        VirtualKeyCode::Caret => Some(30),
        VirtualKeyCode::Slash => Some(31),    // Ctrl+_
        _ => None,
    }
}

pub struct RoseWindow {
    window: winit::window::Window,
    pixels: Pixels,
    pty: Pty,
    terminal: Terminal,
    parser: Parser,
    renderer: FontRenderer,
    clipboard: Clipboard,
    is_selecting: bool,

    // Key Repeat State
    last_key: Option<VirtualKeyCode>,
    repeat_deadline: Instant,
}

impl RoseWindow {
    pub fn new(event_loop: &EventLoop<RoseEvent>) -> Result<Self> {
        let size = LogicalSize::new(800.0, 600.0);
        let window = WindowBuilder::new()
            .with_title("RoseTerm")
            .with_inner_size(size)
            .build(event_loop)?;

        let window_size = window.inner_size();
        let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
        let pixels = Pixels::new(window_size.width, window_size.height, surface_texture)?;

        let renderer = FontRenderer::new()?;

        let cols = (window_size.width as f32 / renderer.char_width) as usize;
        let rows = (window_size.height as f32 / renderer.char_height) as usize;

        let terminal = Terminal::new(cols, rows);
        let proxy = event_loop.create_proxy();
        let pty = Pty::spawn(proxy, cols as u16, rows as u16)?;
        let parser = Parser::new();
        let clipboard = Clipboard::new()?;

        Ok(Self {
            window,
            pixels,
            pty,
            terminal,
            parser,
            renderer,
            clipboard,
            is_selecting: false,

            last_key: None,
            repeat_deadline: Instant::now(),
        })
    }

    pub fn draw(&mut self) {
        let width = self.window.inner_size().width;
        let frame = self.pixels.frame_mut();
        self.renderer.draw(&self.terminal, frame, width);
        if let Err(e) = self.pixels.render() {
            error!("pixels.render() failed: {}", e);
        }
    }

    // Helper to send special keys (Arrows, Home, End, etc)
    fn process_special_key(&mut self, key: VirtualKeyCode, held_shift: bool, held_ctrl: bool) -> bool {
        match key {
            VirtualKeyCode::Return => {
                if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
                // FIX: Send \r (Carriage Return) instead of \n
                let _ = self.pty.writer.write_all(b"\r");
                true
            }
            VirtualKeyCode::Escape => {
                let _ = self.pty.writer.write_all(b"\x1b");
                true
            }
            VirtualKeyCode::Back => {
                if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
                let _ = self.pty.writer.write_all(b"\x7f");
                true
            }
            VirtualKeyCode::Delete => {
                let _ = self.pty.writer.write_all(b"\x1b[3~");
                true
            }

            // ARROWS
            VirtualKeyCode::Up => {
                if held_shift && !held_ctrl { self.terminal.scroll_up(1); }
                else { let _ = self.pty.writer.write_all(b"\x1b[A"); }
                true
            }
            VirtualKeyCode::Down => {
                if held_shift && !held_ctrl { self.terminal.scroll_down(1); }
                else { let _ = self.pty.writer.write_all(b"\x1b[B"); }
                true
            }
            VirtualKeyCode::Right => { let _ = self.pty.writer.write_all(b"\x1b[C"); true }
            VirtualKeyCode::Left => { let _ = self.pty.writer.write_all(b"\x1b[D"); true }

            // NAVIGATION
            VirtualKeyCode::PageUp => {
                if held_shift { self.terminal.scroll_up(10); }
                else { let _ = self.pty.writer.write_all(b"\x1b[5~"); }
                true
            }
            VirtualKeyCode::PageDown => {
                if held_shift { self.terminal.scroll_down(10); }
                else { let _ = self.pty.writer.write_all(b"\x1b[6~"); }
                true
            }
            VirtualKeyCode::Home => { let _ = self.pty.writer.write_all(b"\x1b[H"); true }
            VirtualKeyCode::End => { let _ = self.pty.writer.write_all(b"\x1b[F"); true }

            _ => false
        }
    }

    pub fn handle_input(&mut self, input: &WinitInputHelper) {
        let is_copy_paste_hotkey = input.held_control() && input.held_shift();

        // 1. Handle Regular Text (No Control held)
        if !input.held_control() && !input.held_alt() {
            if !input.text().is_empty() {
               for text_char in input.text() {
                   if let TextChar::Char(c) = text_char {
                       let mut bytes = [0; 4];
                       let s = c.encode_utf8(&mut bytes);
                       if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
                       let _ = self.pty.writer.write_all(s.as_bytes());
                   }
               }
            }
        }

        // 2. Handle CONTROL CODES (Ctrl+A ... Ctrl+Z)
        if input.held_control() && !is_copy_paste_hotkey {
            let keys = [
                VirtualKeyCode::A, VirtualKeyCode::B, VirtualKeyCode::C, VirtualKeyCode::D, VirtualKeyCode::E,
                VirtualKeyCode::F, VirtualKeyCode::G, VirtualKeyCode::H, VirtualKeyCode::I, VirtualKeyCode::J,
                VirtualKeyCode::K, VirtualKeyCode::L, VirtualKeyCode::M, VirtualKeyCode::N, VirtualKeyCode::O,
                VirtualKeyCode::P, VirtualKeyCode::Q, VirtualKeyCode::R, VirtualKeyCode::S, VirtualKeyCode::T,
                VirtualKeyCode::U, VirtualKeyCode::V, VirtualKeyCode::W, VirtualKeyCode::X, VirtualKeyCode::Y,
                VirtualKeyCode::Z, VirtualKeyCode::LBracket, VirtualKeyCode::RBracket, VirtualKeyCode::Backslash
            ];

            for key in keys {
                if input.key_pressed(key) {
                    if let Some(byte) = ctrl_key_to_byte(key) {
                        if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
                        let _ = self.pty.writer.write_all(&[byte]);
                    }
                }
            }
        }

        // 3. Handle Key Repeats for Special Keys
        let mut handled_special = false;
        let keys_to_check = [
            VirtualKeyCode::Return, VirtualKeyCode::Escape, VirtualKeyCode::Back, VirtualKeyCode::Delete,
            VirtualKeyCode::Up, VirtualKeyCode::Down, VirtualKeyCode::Left, VirtualKeyCode::Right,
            VirtualKeyCode::PageUp, VirtualKeyCode::PageDown, VirtualKeyCode::Home, VirtualKeyCode::End
        ];

        for &key in &keys_to_check {
            if input.key_pressed(key) {
                self.process_special_key(key, input.held_shift(), input.held_control());
                self.last_key = Some(key);
                self.repeat_deadline = Instant::now() + Duration::from_millis(500);
                handled_special = true;
                break;
            }
        }

        if !handled_special {
            if let Some(key) = self.last_key {
                if input.key_held(key) {
                    if Instant::now() >= self.repeat_deadline {
                        self.process_special_key(key, input.held_shift(), input.held_control());
                        self.repeat_deadline = Instant::now() + Duration::from_millis(50);
                    }
                } else {
                    self.last_key = None;
                }
            }
        }

        // --- COPY / PASTE ---
        if input.held_shift() && input.key_pressed(VirtualKeyCode::Insert) {
             if let Ok(text) = self.clipboard.get_text() {
                 if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
                 let _ = self.pty.writer.write_all(text.as_bytes());
             }
        }

        if input.held_control() && input.held_shift() {
            if input.key_pressed(VirtualKeyCode::C) {
                let text = self.terminal.get_selected_text();
                if !text.is_empty() { let _ = self.clipboard.set_text(text); }
            }
            if input.key_pressed(VirtualKeyCode::V) {
                if let Ok(text) = self.clipboard.get_text() {
                    let _ = self.pty.writer.write_all(text.as_bytes());
                }
            }
        }

        // --- MOUSE HANDLING ---
        if let Some((mx, my)) = input.mouse() {
            let col = (mx / self.renderer.char_width) as usize;
            let row = (my / self.renderer.char_height) as usize;

            let force_selection = input.held_shift();
            let app_mouse_mode = self.terminal.mouse_reporting && !force_selection;

            if app_mouse_mode {
                if input.mouse_pressed(0) {
                    let _ = self.pty.writer.write_all(encode_mouse(0, col, row, false).as_bytes());
                }
                if input.mouse_released(0) {
                    let _ = self.pty.writer.write_all(encode_mouse(0, col, row, true).as_bytes());
                }
                if input.mouse_pressed(1) {
                    let _ = self.pty.writer.write_all(encode_mouse(2, col, row, false).as_bytes());
                }
                let scroll = input.scroll_diff();
                if scroll > 0.0 {
                    let _ = self.pty.writer.write_all(encode_mouse(64, col, row, false).as_bytes());
                } else if scroll < 0.0 {
                      let _ = self.pty.writer.write_all(encode_mouse(65, col, row, false).as_bytes());
                }
            } else {
                if input.mouse_pressed(0) {
                    self.is_selecting = true;
                    self.terminal.start_selection(col, row);
                    self.window.request_redraw();
                }

                if self.is_selecting {
                    self.terminal.update_selection(col, row);
                    self.window.request_redraw();
                }

                if input.mouse_released(0) {
                    self.is_selecting = false;
                    if self.terminal.selection_start == self.terminal.selection_end {
                        self.terminal.clear_selection();
                        self.window.request_redraw();
                    }
                }

                if input.mouse_released(1) {
                    let text = self.terminal.get_selected_text();
                    if !text.is_empty() {
                        let _ = self.clipboard.set_text(text);
                        self.terminal.clear_selection();
                        self.window.request_redraw();
                    }
                }

                let scroll = input.scroll_diff();
                if scroll > 0.0 { self.terminal.scroll_up(3); self.window.request_redraw(); }
                else if scroll < 0.0 { self.terminal.scroll_down(3); self.window.request_redraw(); }
            }
        }
    }

    pub fn on_pty_data(&mut self, data: Vec<u8>) {
        for byte in data {
            self.parser.advance(&mut self.terminal, byte);
        }
        self.window.set_title(&self.terminal.title);
    }
}

pub fn run() -> Result<()> {
    let event_loop = EventLoop::with_user_event();
    let mut app = RoseWindow::new(&event_loop)?;
    let mut input = WinitInputHelper::new();

    event_loop.run(move |event, _, control_flow| {
        // Smart wait logic
        if app.last_key.is_some() {
             *control_flow = ControlFlow::Poll;
        } else {
             *control_flow = ControlFlow::Wait;
        }

        if let Event::RedrawRequested(_) = event {
            app.draw();
        }

        match event {
            Event::UserEvent(RoseEvent::Exit) => {
                *control_flow = ControlFlow::Exit;
                return;
            }
            Event::UserEvent(RoseEvent::PtyOutput(ref data)) => {
                 app.on_pty_data(data.clone());
                 app.window.request_redraw();
            }
            _ => {}
        }

        if input.update(&event) {
            if input.close_requested() {
                *control_flow = ControlFlow::Exit;
                return;
            }

            if let Some(size) = input.window_resized() {
                let _ = app.pixels.resize_surface(size.width, size.height);
                let _ = app.pixels.resize_buffer(size.width, size.height);
                let cols = (size.width as f32 / app.renderer.char_width) as usize;
                let rows = (size.height as f32 / app.renderer.char_height) as usize;
                if cols > 0 && rows > 0 {
                    app.terminal.resize(cols, rows);
                    let _ = app.pty.resize(rows as u16, cols as u16);
                }
                app.window.request_redraw();
            }

            app.handle_input(&input);
            if app.terminal.scroll_offset > 0 || input.held_shift() || app.is_selecting {
                app.window.request_redraw();
            }
        }
    });
}