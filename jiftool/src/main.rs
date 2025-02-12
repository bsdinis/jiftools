//! # `jiftool`
//!
//! A tool for modifying JIF files
//!
//! Example usage:
//! ```sh
//! $ jiftool orig.jif terse.jif # remove duplicate strings, etc.
//! $ jiftool orig.jif new.jif rename /usr/bin/ld.so /bin/ld.so # rename path to `ld.so`
//! $ jiftool orig.jif itree.jif build-itrees # build interval trees
//! $ jiftool orig.jif ordered.jif add-ord tsa.ord # add an ordering section
//! ```
use jif::*;
use tracer_format::{dedup_and_sort_by_addr, read_trace};

use anyhow::Context;
use clap::Parser;
use std::fs::File;
use std::io::{BufReader, BufWriter};

mod tsa;
use tsa::*;

#[derive(Parser)]
#[command(version, about, long_about = None)]
/// Modify JIF files
struct Cli {
    /// Input file path
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    input_file: std::path::PathBuf,

    /// Output file path
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    output_file: std::path::PathBuf,

    /// Whether to print out the resulting JIF
    #[arg(long)]
    show: bool,

    /// Modifying command
    ///
    /// In the absence of a command it will simply
    /// remove duplicate strings and other isomorphic compression techniques
    command: Vec<String>,
}

#[derive(Parser, Debug)]
enum Command {
    /// Rename a referenced file in the JIF
    Rename {
        /// Old name
        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        old_path: String,

        /// New name
        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        new_path: String,
    },
    /// Build the interval trees in the JIF
    BuildItrees {
        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        chroot_path: Option<std::path::PathBuf>,
    },

    /// Fragment VMAs in the JIF, but still finding zero pages and ref segments
    FragmentVmas {
        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        chroot_path: Option<std::path::PathBuf>,
    },

    /// Setup the prefetch section (includes fragmenting the ordering intervals)
    SetupPrefetch,

    /// Tag the VMAs with a bit letting it know whether they are in the ordering section
    TagVmas,

    /// Add an ordering section
    ///
    /// Ingests a timestamped access log (each line of format `<usecs>: <address>`)
    /// to construct the ordering list
    AddOrd {
        /// Filepath of the timestamped access log (defaults to `stdin`)
        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        time_log: Option<std::path::PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    let mut input_file =
        BufReader::new(File::open(&args.input_file).context("failed to open input JIF")?);

    let mut jif = Jif::from_reader(&mut input_file)?;

    for command in args.command {
        let args = std::iter::once("").chain(command.split_whitespace());
        let command = Command::parse_from(args);
        match command {
            Command::Rename { old_path, new_path } => jif.rename_file(&old_path, &new_path),
            Command::BuildItrees { chroot_path } => jif
                .build_itrees(chroot_path)
                .context("failed to build ITrees")?,
            Command::FragmentVmas { chroot_path } => jif
                .fragment_vmas(chroot_path)
                .context("failed to fragment vmas")?,
            Command::SetupPrefetch => jif
                .setup_prefetch()
                .context("failed to setup prefetch section")?,
            Command::TagVmas => jif.tag_vmas(),
            Command::AddOrd { time_log } => {
                let tsa_log = match time_log {
                    Some(fname) => {
                        let file =
                            BufReader::new(File::open(fname).context("failed to open ord list")?);
                        read_trace(file).context("failed to read trace")?
                    }
                    None => {
                        let stdin = std::io::stdin();
                        read_trace(stdin.lock()).context("failed to read trace")?
                    }
                };

                let tsa_log = dedup_and_sort_by_addr(tsa_log);
                let ords = construct_ord_chunks(&jif, tsa_log);

                jif.add_ordering_info(ords)?;
            }
        }
    }

    let mut output_file =
        BufWriter::new(File::create(&args.output_file).context("failed to open output JIF")?);
    let raw = JifRaw::from_materialized(jif);

    if args.show {
        println!("{:#x?}", raw);
    }
    raw.to_writer(&mut output_file)
        .context("failed to write JIF")?;
    Ok(())
}
