#![allow(clippy::excessive_precision, clippy::too_many_arguments)]

//! Slim entry point for `omastudio-engine`; subcommands live in [`cli`], the
//! persistent JSON-lines daemon lives in [`daemon`], Omarchy theme parsing lives
//! in [`theme`], and shared image-processing modules live in `omastudio_engine`.

mod bench;
mod cli;
mod daemon;
mod mcp;
mod theme;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    cli::dispatch(&args);
}
