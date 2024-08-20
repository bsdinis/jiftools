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

const PLOT_UPSET_PY: &str = "
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
    plt.suptitle('Intersection of {} regions among jif snapshots'.format(sys.argv[1]))
    plt.savefig(sys.argv[2])
";

#[derive(Parser, Debug)]
#[command(version)]
/// cmpjif: compare JIF files
///
/// This tool compares JIF files to produce an upset plot (a flat representation of a multi-dimensional Venn Diagram)
///
struct Cli {
    /// JIF file to read from
    #[arg(value_name = "FILE", num_args = 2.., value_hint = clap::ValueHint::FilePath)]
    jif_files: Vec<std::path::PathBuf>,

    /// Consider only the shared pages
    #[arg(short, long, conflicts_with = "private")]
    shared: bool,

    /// Consider only the private pages
    #[arg(short, long, conflicts_with = "shared")]
    private: bool,

    /// Consider only the pages in the ordering segment
    #[arg(short, long)]
    ordering: bool,

    /// Compare only the shared pages
    #[arg(short, long, value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    output: std::path::PathBuf,
}

/// Build a set of hashes of the private pages
fn build_private_pages_hash_set(jif: &Jif) -> HashSet<Sha256Hash> {
    fn sha256_page(page: &[u8]) -> Sha256Hash {
        let mut hasher = Sha256::new();
        hasher.update(page);
        hasher.finalize().into()
    }

    jif.iter_private_pages().map(sha256_page).collect()
}

/// Build a set of hashes of pages
fn build_shared_pages_set(jif: &Jif) -> HashSet<(String, u64, u64)> {
    jif.iter_shared_regions()
        .map(|(string, start, end)| (string.to_string(), start, end))
        .collect()
}

/// Open the JIF file
fn open_jif(path: &std::path::Path) -> anyhow::Result<Jif> {
    Jif::from_reader(&mut BufReader::new(
        File::open(path).context("failed to open file")?,
    ))
    .context("failed to read jif")
}

#[derive(Default)]
struct JifDigest {
    // digest of each private page
    private_pages: HashSet<Sha256Hash>,

    // <pathname, offset> for shared pages
    shared_pages: HashSet<(String, u64, u64)>,
}

/// Plot the intersection between the files
/// Constructs an [upset plot](https://en.wikipedia.org/wiki/UpSet_plot) by shelling out to python
fn plot_intersections(
    digests: HashMap<std::path::PathBuf, JifDigest>,
    plot_title: &str,
    output_filename: PathBuf,
) -> anyhow::Result<()> {
    let mut child = Command::new("python")
        .arg("-c")
        .arg(PLOT_UPSET_PY)
        .arg(plot_title)
        .arg(format!("{}", output_filename.display()))
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to spawn python plotter: make sure the python packages are installed (matplotlib and upsetplot)")?;

    let mut stdin = child
        .stdin
        .take()
        .context("failed to open pipe to plotter")?;

    for (path, digest) in digests {
        stdin
            .write_all(format!("{}: ", path.display()).as_bytes())
            .context("failed to write")?;

        for hash in digest.private_pages {
            let str = hash.map(|byte| format!("{:x}", byte)).join("");
            stdin
                .write_all(format!("private_{}, ", str).as_bytes())
                .context("failed to write")?;
        }

        for (pathname, start, end) in digest.shared_pages {
            stdin
                .write_all(format!("shared_{}:{:x}-{:x}, ", pathname, start, end).as_bytes())
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

            let mut digest = JifDigest::default();

            if !cli.shared {
                digest.private_pages = build_private_pages_hash_set(&jif);
            }

            if !cli.private {
                digest.shared_pages = build_shared_pages_set(&jif);
            }

            Ok::<_, anyhow::Error>((p, digest))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;

    let plot_title = if cli.shared {
        "shared"
    } else if cli.private {
        "private"
    } else {
        "all"
    };
    plot_intersections(hashes, plot_title, cli.output)
}
