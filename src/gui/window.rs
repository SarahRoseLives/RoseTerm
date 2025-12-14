use anyhow::Result;
use log::error;
use pixels::{Pixels, SurfaceTexture};
use winit::{
    dpi::LogicalSize,
    event::{Event, VirtualKeyCode},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder},
    window::WindowBuilder,
};
use winit_input_helper::{WinitInputHelper, TextChar};
use vte::Parser;
use arboard::Clipboard;

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

pub struct RoseWindow {
    window: winit::window::Window,
    pixels: Pixels,
    pty: Pty,
    terminal: Terminal,
    parser: Parser,
    renderer: FontRenderer,
    clipboard: Clipboard,
    is_selecting: bool,
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

    pub fn handle_input(&mut self, input: &WinitInputHelper) {
        // 1. Handle Typing
        if !input.held_control() && !input.held_alt() {
            if !input.text().is_empty() {
               for text_char in input.text() {
                   match text_char {
                       TextChar::Char(c) => {
                           let mut bytes = [0; 4];
                           let s = c.encode_utf8(&mut bytes);
                           if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
                           let _ = self.pty.writer.write_all(s.as_bytes());
                       }
                       TextChar::Back => {
                           if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
                           let _ = self.pty.writer.write_all(b"\x08");
                       }
                   }
               }
            }
        }

        // --- SPECIAL KEYS ---

        // Escape Key (Send \x1b instead of closing window)
        if input.key_pressed(VirtualKeyCode::Escape) {
            let _ = self.pty.writer.write_all(b"\x1b");
        }

        // Return Key
        if input.key_pressed(VirtualKeyCode::Return) {
            if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
            let _ = self.pty.writer.write_all(b"\n");
        }

        // Ctrl + C (Interrupt)
        if input.held_control() && input.key_pressed(VirtualKeyCode::C) && !input.held_shift() {
             if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
             let _ = self.pty.writer.write_all(&[0x03]);
        }

        // Ctrl + D (EOF)
        if input.held_control() && input.key_pressed(VirtualKeyCode::D) && !input.held_shift() {
             let _ = self.pty.writer.write_all(&[0x04]);
        }

        // Paste: Shift + Insert
        if input.held_shift() && input.key_pressed(VirtualKeyCode::Insert) {
             if let Ok(text) = self.clipboard.get_text() {
                 if self.terminal.scroll_offset > 0 { self.terminal.scroll_offset = 0; }
                 let _ = self.pty.writer.write_all(text.as_bytes());
             }
        }

        // Copy/Paste (Ctrl+Shift+C/V)
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

        // Navigation (Arrows + PageUp/Down)
        if input.held_shift() && !input.held_control() {
            if input.key_pressed(VirtualKeyCode::PageUp) { self.terminal.scroll_up(10); }
            if input.key_pressed(VirtualKeyCode::PageDown) { self.terminal.scroll_down(10); }
            if input.key_pressed(VirtualKeyCode::Up) { self.terminal.scroll_up(1); }
            if input.key_pressed(VirtualKeyCode::Down) { self.terminal.scroll_down(1); }
        } else {
            if input.key_pressed(VirtualKeyCode::Up) { let _ = self.pty.writer.write_all(b"\x1b[A"); }
            if input.key_pressed(VirtualKeyCode::Down) { let _ = self.pty.writer.write_all(b"\x1b[B"); }
            if input.key_pressed(VirtualKeyCode::Right) { let _ = self.pty.writer.write_all(b"\x1b[C"); }
            if input.key_pressed(VirtualKeyCode::Left) { let _ = self.pty.writer.write_all(b"\x1b[D"); }
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
    let event_loop = EventLoopBuilder::<RoseEvent>::with_user_event().build();
    let mut app = RoseWindow::new(&event_loop)?;
    let mut input = WinitInputHelper::new();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

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
            // FIX: Only close on explicit close request (clicking X), NOT on Escape
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