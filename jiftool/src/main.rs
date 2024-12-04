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
use tracer_format::{dedup_and_sort, read_trace};

use anyhow::Context;
use clap::{Parser, Subcommand};
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
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
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
    Fragment {
        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        chroot_path: Option<std::path::PathBuf>,
    },

    /// Add an ordering section
    ///
    /// Ingests a timestamped access log (each line of format `<usecs>: <address>`)
    /// to construct the ordering list
    AddOrd {
        /// Filepath of the timestamped access log (defaults to `stdin`)
        #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
        time_log: Option<std::path::PathBuf>,

        // True if doing prefetch setup (breaking intervals per ord chunks).
        #[arg(long)]
        setup_prefetch: bool,

        // fragment the itrees
        #[arg(long)]
        fragment: bool,

        // fragment itrees into different vams
        #[arg(long, value_name = "FILE", value_hint = clap::ValueHint::FilePath, num_args = 0..=1, default_missing_value = None)]
        chroot: Option<std::path::PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    let mut input_file =
        BufReader::new(File::open(&args.input_file).context("failed to open input JIF")?);

    let mut jif = Jif::from_reader(&mut input_file)?;

    let mut reorder = false;
    match args.command {
        None => {}
        Some(Command::Rename { old_path, new_path }) => jif.rename_file(&old_path, &new_path),
        Some(Command::BuildItrees { chroot_path }) => jif
            .build_itrees(chroot_path)
            .context("failed to build ITrees")?,
        Some(Command::Fragment { chroot_path }) => jif
            .fragment(chroot_path)
            .context("failed to fragment vmas")?,
        Some(Command::AddOrd {
            time_log,
            setup_prefetch,
            fragment,
            chroot,
        }) => {
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

            let tsa_log = dedup_and_sort(tsa_log);
            let ords = construct_ord_chunks(&jif, tsa_log);
            reorder = setup_prefetch;

            jif.add_ordering_info(ords)?;
            if fragment {
                jif.fragment(chroot)?;
            }
        }
    }

    let mut output_file =
        BufWriter::new(File::create(&args.output_file).context("failed to open output JIF")?);
    let raw = JifRaw::from_materialized(jif, reorder);

    if args.show {
        println!("{:#x?}", raw);
    }
    raw.to_writer(&mut output_file)
        .context("failed to write JIF")?;
    Ok(())
}
