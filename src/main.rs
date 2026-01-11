use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;

use cli::*;

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        Command::Init => commands::init::execute(),
        Command::CatFile {
            pretty_print,
            object_hash,
        } => commands::cat_file::execute(pretty_print, object_hash),
        Command::HashObject { write, file } => {
            commands::hash_object::execute(write, file)
        }
        Command::LsTree { name_only } => commands::name_only::execute(name_only)
    }
}
