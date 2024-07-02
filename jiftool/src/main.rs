use jif::*;

use clap::{Parser, Subcommand};
use std::fs::File;
use std::io::{BufReader, BufWriter};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    input_file: std::path::PathBuf,

    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    output_file: std::path::PathBuf,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Rename {
        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        old_path: String,

        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        new_path: String,
    },
}

fn main() -> JifResult<()> {
    let args = Cli::parse();
    let mut input_file = BufReader::new(File::open(&args.input_file)?);

    let mut jif = Jif::from_reader(&mut input_file)?;

    match args.command {
        None => {}
        Some(Command::Rename { old_path, new_path }) => jif.rename_file(&old_path, &new_path),
    }

    let mut output_file = BufWriter::new(File::create(&args.output_file)?);
    let raw = JifRaw::from_materialized(jif);
    eprintln!("{:#x?}", raw);
    raw.to_writer(&mut output_file)?;
    Ok(())
}
