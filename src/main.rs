mod app;
mod cli;
mod config;
mod git;
mod github;
mod highlighting;
mod models;
mod ui;

use anyhow::Result;
use clap::Parser;

slint::include_modules!();

fn main() -> Result<()> {
    let args = cli::Args::parse();

    // Handle shell completion generation
    if let Some(shell) = args.completions {
        cli::generate_completions(shell);
        return Ok(());
    }

    let app = app::App::new(args)?;
    app.run()?;

    Ok(())
}
