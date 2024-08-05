//! # `tracejif`
//!
//! A tool to add context to a memory trace
//!
//! Example usage:
//! ```sh
//! $ tracejif a.jif a.ord
//! ```

use jif::*;
use tracer_format::*;

use jif::itree::interval::DataSource;

use std::fs::File;
use std::io::BufReader;

use anyhow::Context;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version)]
/// tracejif: add context to a memory trace from junction
struct Cli {
    /// JIF file to read from
    #[arg(value_hint = clap::ValueHint::FilePath)]
    jif_file: std::path::PathBuf,

    /// Ordering file outputted by junction_run --trace
    #[arg(value_hint = clap::ValueHint::FilePath)]
    ord_file: std::path::PathBuf,
}

/// Print the trace
fn print_trace(jif: &Jif, tsa: &[TimestampedAccess]) {
    for entry in tsa {
        let data_source = match jif.resolve(entry.addr as u64).map(|ival| ival.source) {
            Some(DataSource::Zero) => "zero",
            Some(DataSource::Private) => "private",
            Some(DataSource::Shared) => "shared",
            None => "unknown",
        };
        if let Some(pheader) = jif.mapping_pheader(entry.addr as u64) {
            println!(
                "{}: {:#x?} | {:#x?}-{:#x?} | {} | {}",
                entry.usecs,
                entry.addr,
                pheader.virtual_range().0,
                pheader.virtual_range().1,
                pheader.pathname().unwrap_or("<unnamed>"),
                data_source
            );
        } else {
            println!("{}: {:#x?} | {}", entry.usecs, entry.addr, data_source);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let jif = Jif::from_reader(&mut BufReader::new(
        File::open(cli.jif_file).context("failed to open file")?,
    ))
    .context("failed to read jif")?;

    let trace = {
        let file = BufReader::new(File::open(cli.ord_file).context("failed to open ord list")?);
        let trace = read_trace(file).context("failed to read the trace")?;

        Ok::<Vec<TimestampedAccess>, anyhow::Error>(dedup_and_sort(trace))
    }?;

    print_trace(&jif, &trace);
    Ok(())
}
