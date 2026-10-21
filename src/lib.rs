//! Core library for the `als` command.

pub mod bash;
pub mod fish;
pub mod model;
pub mod store;
pub mod zsh;

mod cli;
mod interactive;
mod selector;
mod table;

/// Parses command-line arguments and runs `als`.
pub fn run() -> anyhow::Result<()> {
    cli::run()
}
