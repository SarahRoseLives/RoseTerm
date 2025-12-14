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

use crate::backend::pty::Pty;
use crate::terminal::grid::Terminal;
use crate::renderer::font::FontRenderer;

#[derive(Debug)]
pub enum RoseEvent {
    PtyOutput(Vec<u8>),
}

pub struct RoseWindow {
    window: winit::window::Window,
    pixels: Pixels,
    pty: Pty,
    terminal: Terminal,
    parser: Parser,
    renderer: FontRenderer,
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

        // 1. Create Renderer first so we know char dimensions
        let renderer = FontRenderer::new()?;

        // 2. Calculate initial Rows/Cols based on window size
        let cols = (window_size.width as f32 / renderer.char_width) as usize;
        let rows = (window_size.height as f32 / renderer.char_height) as usize;

        // 3. Create Terminal and PTY with correct size
        let terminal = Terminal::new(cols, rows);

        // We need the proxy to create the PTY
        let proxy = event_loop.create_proxy();
        let pty = Pty::spawn(proxy, cols as u16, rows as u16)?;

        let parser = Parser::new();

        Ok(Self {
            window,
            pixels,
            pty,
            terminal,
            parser,
            renderer,
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
        if !input.text().is_empty() {
           for text_char in input.text() {
               match text_char {
                   TextChar::Char(c) => {
                       let mut bytes = [0; 4];
                       let s = c.encode_utf8(&mut bytes);
                       let _ = self.pty.writer.write_all(s.as_bytes());
                   }
                   TextChar::Back => {
                       let _ = self.pty.writer.write_all(b"\x08");
                   }
               }
           }
        }

        if input.key_pressed(VirtualKeyCode::Return) {
             let _ = self.pty.writer.write_all(b"\n");
        }
        if input.key_pressed(VirtualKeyCode::Up) { let _ = self.pty.writer.write_all(b"\x1b[A"); }
        if input.key_pressed(VirtualKeyCode::Down) { let _ = self.pty.writer.write_all(b"\x1b[B"); }
        if input.key_pressed(VirtualKeyCode::Right) { let _ = self.pty.writer.write_all(b"\x1b[C"); }
        if input.key_pressed(VirtualKeyCode::Left) { let _ = self.pty.writer.write_all(b"\x1b[D"); }
    }

    pub fn on_pty_data(&mut self, data: Vec<u8>) {
        for byte in data {
            self.parser.advance(&mut self.terminal, byte);
        }
    }
}

pub fn run() -> Result<()> {
    let event_loop = EventLoopBuilder::<RoseEvent>::with_user_event().build();

    // Init Window (Pty is now created inside new)
    let mut app = RoseWindow::new(&event_loop)?;
    let mut input = WinitInputHelper::new();

    event_loop.run(move |event, _, control_flow| {
        if let Event::RedrawRequested(_) = event {
            app.draw();
        }

        if let Event::UserEvent(RoseEvent::PtyOutput(ref data)) = event {
             app.on_pty_data(data.clone());
             app.window.request_redraw();
        }

        if input.update(&event) {
            if input.key_pressed(VirtualKeyCode::Escape) || input.close_requested() {
                *control_flow = ControlFlow::Exit;
                return;
            }

            // HANDLE RESIZE HERE
            if let Some(size) = input.window_resized() {
                let _ = app.pixels.resize_surface(size.width, size.height);
                let _ = app.pixels.resize_buffer(size.width, size.height);

                // 1. Calculate new grid size
                let cols = (size.width as f32 / app.renderer.char_width) as usize;
                let rows = (size.height as f32 / app.renderer.char_height) as usize;

                if cols > 0 && rows > 0 {
                    // 2. Resize internal grid
                    app.terminal.resize(cols, rows);

                    // 3. Tell the Shell (PTY) the new size!
                    let _ = app.pty.resize(rows as u16, cols as u16);
                }

                app.window.request_redraw();
            }

            app.handle_input(&input);
        }
    });
}