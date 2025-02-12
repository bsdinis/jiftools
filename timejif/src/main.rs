//! # `timejif`
//!
//! A tool to plot the timing behaviour given an ordering section
//!
//! Example usage:
//! ```sh
//! $ timejif a.jif a.ord
//! ```

use jif::*;
use tracer_format::*;

use jif::itree::interval::DataSource;

use std::fs::File;
use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::Context;
use clap::Parser;

const PLOT_TIME_PY: &str = "
import matplotlib.pyplot as plt
import sys

if __name__ == '__main__':
    if len(sys.argv) != 3:
        sys.exit('usage: ./plot_time.py <output filename> <plot title>')

    output='{}.pdf'.format(sys.argv[1])
    title=sys.argv[2]

    all_x = list()
    all_y = list()

    non_shared_x = list()
    non_shared_y = list()

    private_x = list()
    private_y = list()

    private_cnt = 0
    zero_cnt = 0
    shared_cnt = 0
    for line in sys.stdin.readlines():
        split = line.strip().split(' ')
        assert len(split) == 2, 'expected format is <timestamp> <\\'zero\\' | \\'shared\\' | \\'private\\' | \\'unknown\\'>'

        timestamp_ms = float(split[0])
        type = split[1]

        all_x.append(timestamp_ms)
        all_y.append(len(all_x))
        if type == 'private':
            non_shared_x.append(timestamp_ms)
            non_shared_y.append(len(non_shared_x))
            private_x.append(timestamp_ms)
            private_y.append(len(private_x))
            private_cnt += 1
        elif type == 'zero':
            non_shared_x.append(timestamp_ms)
            non_shared_y.append(len(non_shared_x))
            zero_cnt += 1
        elif type == 'shared':
            shared_cnt += 1

    plt.scatter(all_x, all_y, s=5, label='all')
    plt.scatter(non_shared_x, non_shared_y, s=5, label='private')
    plt.scatter(private_x, private_y, s=5, label='private - zero')

    plt.xlabel('Time (ms)', fontfamily='sans-serif', fontsize=12)
    plt.ylabel('Number of unique pages', fontfamily='sans-serif', fontsize=12)
    plt.title(title, fontfamily='sans-serif', fontsize=15)
    plt.legend()
    plt.savefig(output)
    print('{}, \\t{}, \\t{}, \\t{}, \\t{}'.format(title, len(all_x), private_cnt, shared_cnt, zero_cnt))
";

#[derive(Parser, Debug)]
#[command(version)]
/// timejif: plot timing information about first faults of pages
struct Cli {
    /// JIF file to read from
    #[arg(value_hint = clap::ValueHint::FilePath)]
    jif_file: std::path::PathBuf,

    /// Ordering file outputted by junction_run --trace
    #[arg(value_hint = clap::ValueHint::FilePath)]
    ord_file: std::path::PathBuf,

    /// Output file
    #[arg(value_hint = clap::ValueHint::FilePath)]
    output_file: std::path::PathBuf,

    /// Title of the plot
    #[arg(long)]
    title: Option<String>,
}

/// Plot the time plot
fn plot_timeplot(
    jif: &Jif,
    tsa: &[TimestampedAccess],
    title: String,
    output_filename: PathBuf,
) -> anyhow::Result<()> {
    let mut child = Command::new("python3")
        .arg("-c")
        .arg(PLOT_TIME_PY)
        .arg(format!("{}", output_filename.display()))
        .arg(title)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("failed to spawn python plotter: make sure the python packages are installed (matplotlib)")?;

    {
        let mut stdin = child
            .stdin
            .take()
            .context("failed to open pipe to plotter")?;

        for entry in tsa {
            let timestamp_ms = entry.usecs as f64 / 1000.0;

            let data_source = match jif.resolve(entry.addr as u64).map(|ival| ival.source) {
                Some(DataSource::Zero) => "zero",
                Some(DataSource::Private) => "private",
                Some(DataSource::Shared) => "shared",
                None => "unknown",
            };
            stdin.write_all(format!("{} {}\n", timestamp_ms, data_source).as_bytes())?;
        }
    }

    let output = child
        .wait_with_output()
        .context("failed to execute python plotter")?;
    print!(
        "{}",
        String::from_utf8(output.stdout).context("unparseable output")?
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let jif = Jif::from_reader(&mut BufReader::new(
        File::open(cli.jif_file).context("failed to open file")?,
    ))
    .context("failed to read jif")?;

    let default_title = cli
        .ord_file
        .file_stem()
        .and_then(|x| x.to_str().map(|y| y.to_string()))
        .unwrap_or_else(|| "<default>".to_string());

    let trace = {
        let file = BufReader::new(File::open(cli.ord_file).context("failed to open ord list")?);
        let trace = read_trace(file).context("failed to read the trace")?;

        Ok::<Vec<TimestampedAccess>, anyhow::Error>(dedup_and_sort_by_addr(trace))
    }?;

    plot_timeplot(
        &jif,
        &trace,
        cli.title.unwrap_or(default_title),
        cli.output_file,
    )
}
