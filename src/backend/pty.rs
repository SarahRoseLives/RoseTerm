use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem, MasterPty};
use std::{
    io::{Read, Write},
    thread,
};
use winit::event_loop::EventLoopProxy;
use crate::gui::window::RoseEvent;

pub struct Pty {
    pub writer: Box<dyn Write + Send>,
    pub master: Box<dyn MasterPty + Send>, // We hold this to resize later
}

impl Pty {
    pub fn spawn(proxy: EventLoopProxy<RoseEvent>, cols: u16, rows: u16) -> Result<Self> {
        let pty_system = NativePtySystem::default();

        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
        let cmd = CommandBuilder::new(shell);
        pair.slave.spawn_command(cmd)?;

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let master = pair.master; // Move master to struct

        thread::spawn(move || {
            let mut buffer = [0u8; 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => {
                        let bytes = buffer[..n].to_vec();
                        let _ = proxy.send_event(RoseEvent::PtyOutput(bytes));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { writer, master })
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }
}