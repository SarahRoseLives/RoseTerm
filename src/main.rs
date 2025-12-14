mod backend;
mod gui;
mod terminal; // Add this
mod renderer; // Add this

use anyhow::Result;

fn main() -> Result<()> {
    env_logger::init();
    gui::window::run()
}