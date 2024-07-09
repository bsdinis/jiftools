//! # `cmpjif`
//!
//! A tool to compare JIF files, in particular the intersection of non-zero pages
//!
//! Example usage:
//! ```sh
//! $ cmpjif a.jif b.jif # compare a.jif and b.jif
//! # cmpjif --private a.jif b.jif c.jif # compare a.jif, b.jif and c.jif, comparing only the private pages
//! # cmpjif --shared a.jif b.jif c.jif # compare a.jif, b.jif and c.jif, comparing only the shared pages
//! ```

use jif::*;

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::Context;
use clap::Parser;
use sha2::{Digest, Sha256};

type Sha256Hash = [u8; 32];

const PLOT_UPSET_PY: &'static str = "
import matplotlib.pyplot as plt
import upsetplot
import sys

if __name__ == '__main__':
    data = dict()
    for line in sys.stdin.readlines():
        split_colon = line.strip().split(':')
        assert len(split_colon) == 2, 'expected format is <filename>: [<hashes>, ]'

        filename = split_colon[0]
        hashes = set( a.strip() for a in split_colon[1].strip().split(',') if len(a) > 0)

        data[filename] = hashes

    upset_data = upsetplot.from_contents(data)
    upset = upsetplot.plot(upset_data, show_counts='{:,}')
    plt.suptitle('Intersection of private data among jif snapshots')
    plt.savefig(sys.argv[1])
";

#[derive(Parser, Debug)]
#[command(version)]
/// readjif: read and query JIF files
///
/// Thie tool parses the JIF (optionally materializing it) and allows for querying and viewing the
/// JIF
struct Cli {
    /// JIF file to read from
    #[arg(value_name = "FILE", num_args = 2.., value_hint = clap::ValueHint::FilePath)]
    jif_files: Vec<std::path::PathBuf>,

    /// Compare only the shared pages
    #[arg(short, long, value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    output: std::path::PathBuf,
}

/// Build a multi set (set with count) of hashes of pages
fn build_hash_multiset(jif: &Jif) -> HashSet<Sha256Hash> {
    fn sha256_page(page: &[u8]) -> Sha256Hash {
        let mut hasher = Sha256::new();
        hasher.update(page);
        hasher.finalize().into()
    }

    jif.iter_private_pages().map(sha256_page).collect()
}

fn open_jif(path: &std::path::Path) -> anyhow::Result<Jif> {
    Jif::from_reader(&mut BufReader::new(
        File::open(path).context("failed to open file")?,
    ))
    .context("failed to read jif")
}

fn plot_intersections(
    hashes: HashMap<std::path::PathBuf, HashSet<Sha256Hash>>,
    output_filename: PathBuf,
) -> anyhow::Result<()> {
    let mut child = Command::new("python")
        .arg("-c")
        .arg(PLOT_UPSET_PY)
        .arg(format!("{}", output_filename.display()))
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to spawn python plotter: make sure the python packages are installed (matplotlib and upsetplot)")?;

    let mut stdin = child
        .stdin
        .take()
        .context("failed to open pipe to plotter")?;

    for (path, hashset) in hashes {
        stdin
            .write_all(format!("{}: ", path.display()).as_bytes())
            .context("failed to write")?;

        for hash in hashset {
            for byte in hash {
                stdin
                    .write_all(format!("{:x}", byte).as_bytes())
                    .context("failed to write")?;
            }
            stdin
                .write_all(", ".as_bytes())
                .context("failed to write")?;
        }
        stdin
            .write_all("\n".as_bytes())
            .context("failed to write")?;
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let hashes = cli
        .jif_files
        .into_iter()
        .map(|p| {
            let jif = open_jif(&p)?;
            Ok::<_, anyhow::Error>((p, build_hash_multiset(&jif)))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;

    plot_intersections(hashes, cli.output)
}
