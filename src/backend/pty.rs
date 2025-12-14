use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use std::{
    io::{Read, Write},
    thread,
};
use winit::event_loop::EventLoopProxy;
use crate::gui::window::RoseEvent;

pub struct Pty {
    pub writer: Box<dyn Write + Send>,
}

impl Pty {
    pub fn spawn(proxy: EventLoopProxy<RoseEvent>) -> Result<Self> {
        let pty_system = NativePtySystem::default();

        // 1. Open PTY with a default size
        let pair = pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // 2. Spawn Shell (Linux)
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
        let cmd = CommandBuilder::new(shell);
        pair.slave.spawn_command(cmd)?;

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        // 3. Spawn a background thread to read from the PTY
        // When data arrives, we send it to the GUI via the proxy
        thread::spawn(move || {
            let mut buffer = [0u8; 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let bytes = buffer[..n].to_vec();
                        // Send data to the GUI event loop
                        let _ = proxy.send_event(RoseEvent::PtyOutput(bytes));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { writer })
    }
}