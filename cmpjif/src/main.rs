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

use jif::itree::interval::DataSource;
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
    #[arg(long)]
    ordering: bool,

    /// Do full analysis (skip printing out)
    #[arg(
        short,
        long,
        conflicts_with = "private",
        conflicts_with = "shared",
        conflicts_with = "output"
    )]
    full: bool,

    /// Compare only the shared pages
    #[arg(short, long, value_name = "FILE", required_unless_present = "full", value_hint = clap::ValueHint::FilePath)]
    output: Option<std::path::PathBuf>,
}

fn sha256_page(page: &[u8]) -> Sha256Hash {
    let mut hasher = Sha256::new();
    hasher.update(page);
    hasher.finalize().into()
}

/// Build a set of hashes of the private pages
fn build_private_pages_hash_set(jif: &Jif) -> HashSet<Sha256Hash> {
    let mut set = HashSet::new();
    jif.for_each_private_page(|page| {
        set.insert(sha256_page(page));
    });
    set
}

/// Build a set of hashes of pages
fn build_shared_pages_set(jif: &Jif) -> HashSet<(String, u64)> {
    jif.iter_shared_regions()
        .flat_map(|(string, start, end)| {
            (start..end)
                .step_by(0x1000)
                .map(|addr| (string.to_string(), addr))
        })
        .collect()
}

/// Build a digest from the ordering section
fn build_ordering_digest(jif: &Jif, include_private: bool, include_shared: bool) -> JifDigest {
    let mut private = Vec::new();
    let mut shared = Vec::new();
    let mut zero_pages = 0;

    for page in jif.ord_chunks().iter().flat_map(|ord| ord.pages()) {
        match jif.resolve(page) {
            None => {
                eprintln!(
                    "{:#x?} is not mapped by the JIF, but is in the ordering segment",
                    page
                );
            }
            Some(interval) => match interval.source {
                DataSource::Zero => {
                    zero_pages += 1;
                }
                DataSource::Shared => {
                    if include_shared {
                        let pheader = jif
                            .mapping_pheader(page)
                            .expect("if the address resolves, it must have a pheader");
                        let offset_into_region = page - pheader.virtual_range().0;
                        let filename = pheader.pathname().expect("if the address resolves into a shared region, it must have a filename").to_string();
                        let ref_offset = pheader.ref_offset().expect("if the address maps to a shared region, it must have a base file offset");
                        shared.push((filename, ref_offset + offset_into_region));
                    }
                }
                DataSource::Private => {
                    if include_private {
                        let borrow = jif.resolve_data(page);
                        let page_data = borrow
                            .get()
                            .expect("if it resolves and is private it must have data");

                        assert_eq!(page_data.len(), 0x1000, "page is not page sized");
                        private.push(sha256_page(page_data));
                    }
                }
            },
        }
    }

    JifDigest {
        private_pages: private.into_iter().collect(),
        shared_pages: shared.into_iter().collect(),
        zero_pages,
    }
}

/// Open the JIF file
fn open_jif(path: &std::path::Path) -> anyhow::Result<Jif> {
    Jif::from_reader(&mut BufReader::new(File::open(path).context(format!(
        "failed to open file {}",
        path.to_str().unwrap_or("<invalid path>")
    ))?))
    .context(format!(
        "failed to read jif {}",
        path.to_str().unwrap_or("<invalid path>")
    ))
}

#[derive(Default, Debug)]
struct JifDigest {
    // digest of each private page
    private_pages: HashSet<Sha256Hash>,

    // <pathname, offset> for shared pages
    shared_pages: HashSet<(String, u64)>,

    // number of zero pages
    zero_pages: usize,
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

        for hash in &digest.private_pages {
            let str = hash.map(|byte| format!("{:x}", byte)).join("");
            stdin
                .write_all(format!("private_{}, ", str).as_bytes())
                .context("failed to write")?;
        }

        //eprintln!("{:?}: {:?}", path, &digest);
        for (pathname, offset) in digest.shared_pages {
            stdin
                .write_all(format!("shared_{}_{:x}, ", pathname, offset).as_bytes())
                .context("failed to write")?;
        }

        stdin
            .write_all("\n".as_bytes())
            .context("failed to write")?;
    }

    Ok(())
}

fn print_intersections(digests: HashMap<std::path::PathBuf, JifDigest>) {
    #[derive(Default, Debug)]
    struct Stats {
        zero_pages: usize,
        private_pages: usize,
        truly_shared_pages: usize,
        unique_shared_pages: usize,
    }

    fn is_unique_shared_page(
        digests: &HashMap<PathBuf, JifDigest>,
        path: &std::path::Path,
        shared_page: &(String, u64),
    ) -> bool {
        for (_path, digest) in digests.iter().filter(|(p, _)| p.as_path() != path) {
            if digest.shared_pages.contains(shared_page) {
                return false;
            }
        }

        true
    }

    fn percentage(parcel: usize, total: usize) -> f64 {
        (parcel * 100) as f64 / total as f64
    }

    let mut stats = HashMap::new();
    for (path, digest) in &digests {
        let mut stat = Stats {
            zero_pages: digest.zero_pages,
            private_pages: digest.private_pages.len(),
            ..Default::default()
        };

        for shared_page in &digest.shared_pages {
            if is_unique_shared_page(&digests, path, shared_page) {
                stat.unique_shared_pages += 1;
            } else {
                stat.truly_shared_pages += 1;
            }
        }

        stats.insert(path, stat);
    }

    let max_width = stats
        .iter()
        .filter_map(|(path, _stat)| path.as_path().to_str().map(|s| s.len()))
        .chain(std::iter::once("filename".len()))
        .max()
        .unwrap_or("filename".len());

    println!(
        "{:^max_width$} | {:^8} | {:^15} | {:^15} | {:^15} | unique but shared |",
        "filename", "total", "zero", "private", "truly shared",
    );
    for (path, stat) in stats {
        let total = stat.zero_pages
            + stat.private_pages
            + stat.truly_shared_pages
            + stat.unique_shared_pages;
        println!(
            "{:max_width$} | {:8} | {:7} ({:4.1}%) | {:7} ({:4.1}%) | {:7} ({:4.1}%) | {:9} ({:4.1}%) |",
            path.as_path().display(),
            total,
            stat.zero_pages,
            percentage(stat.zero_pages, total),
            stat.private_pages,
            percentage(stat.private_pages, total),
            stat.truly_shared_pages,
            percentage(stat.truly_shared_pages, total),
            stat.unique_shared_pages,
            percentage(stat.unique_shared_pages, total)
        );
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let include_private = !cli.shared;
    let include_shared = !cli.private;
    let hashes = cli
        .jif_files
        .into_iter()
        .map(|p| {
            let jif = open_jif(&p)?;

            let digest = if cli.ordering {
                build_ordering_digest(&jif, include_private, include_shared)
            } else {
                let mut digest = JifDigest::default();
                if include_private {
                    digest.private_pages = build_private_pages_hash_set(&jif);
                }

                if include_shared {
                    digest.shared_pages = build_shared_pages_set(&jif);
                }

                digest.zero_pages = jif.zero_pages();

                digest
            };

            Ok::<_, anyhow::Error>((p, digest))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;

    if let Some(output) = cli.output {
        let plot_title = if cli.shared {
            "shared"
        } else if cli.private {
            "private"
        } else {
            "all"
        };
        plot_intersections(hashes, plot_title, output)
    } else {
        print_intersections(hashes);
        Ok(())
    }
}
