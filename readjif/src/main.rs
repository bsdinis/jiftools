//! # `readjif`
//!
//! A tool to read and query JIF files
//!
//! Example usage:
//! ```sh
//! $ readjif a.jif # reads the jif file, dumps a representation of the materialized JIF
//! $ readjif --raw a.jif # reads the jif file, dumps a representation of the raw JIF
//! ```
//!
//!
//! Additionally, there is support for selectively querying the JIF.
//!
//! For materialized JIFs, the API is the following:
//! - `jif`: select the whole JIF
//! - `jif.strings`: strings in the JIF (incompatible with the page selectors)
//! - `jif.zero_pages`: number of zero pages
//! - `jif.private_pages`: number of private pages in the JIF
//! - `jif.shared_pages`: number of shared pages in the pheader
//! - `jif.pages`: total number of pages
//! - `ord`: select all the ord chunks
//! - `ord[<range>]`: select the ord chunks in the range
//! - `ord.len`: number of ord chunks (incompatible with the range selector)
//! - `ord.size`: number of pages in the ordering section (incompatible with the range selector)
//! - `ord.private_pages`: number of private pages in the ordering section
//! - `ord.shared_pages`: number of shared pages in the ordering section
//! - `ord.zero_pages`: number of zero pages in the ordering section
//! - `pheader`: select all the pheaders
//! - `pheader[<range>]`: select the pheaders in the range
//! - `pheader.len`: number of pheaders (incompatible with the range and field selectors)
//! - `pheader.data_size`: size of the data region (mixable with range and other selectors)
//! - `pheader.pathname`: reference pathname (mixable with range and other selectors)
//! - `pheader.ref_offset`: offset into the file
//! - `pheader.virtual_range`: virtual address range of the pheader (mixable with range and other selectors)
//! - `pheader.virtual_size`: size of the virtual address range (mixable with range and other selectors)
//! - `pheader.prot`: area `rwx` protections (mixable with range and other selectors)
//! - `pheader.itree`: pheader interval tree (mixable with range and other selectors)
//! - `pheader.n_itree_nodes`: number of interval tree nodes in pheader (mixable with range and other selectors)
//! - `pheader.zero_pages`: number of zero pages
//! - `pheader.private_pages`: the same as `data_size % PAGE_SIZE`
//! - `pheader.shared_pages`: number of shared pages in the pheader
//! - `pheader.pages`: total number of pages
//!
//! For raw JIFs, the API is similar:
//! - `jif`: select the whole JIF
//! - `jif.data`: size of the data section
//! - `jif.zero_pages`: number of zero pages
//! - `jif.private_pages`: the same as `data % PAGE_SIZE`
//! - `jif.pages`: total number of pages
//! - `strings`: select the strings in the JIF
//! - `itrees`: select all the interval trees
//! - `itrees[<range>]`: select the interval trees in the range
//! - `itrees.len`: number of interval trees (incompatible with the range selector)
//! - `ord`: select all the ord chunks
//! - `ord[<range>]`: select the ord chunks in the range
//! - `ord.len`: number of ord chunks (incompatible with the range selector)
//! - `ord.size`: number of pages in the ordering section (incompatible with the range selector)
//! - `ord.private_pages`: number of private pages in the ordering section
//! - `ord.shared_pages`: number of shared pages in the ordering section
//! - `ord.zero_pages`: number of zero pages in the ordering section
//! - `pheader`: select all the pheaders
//! - `pheader[<range>]`: select the pheaders in the range
//! - `pheader.len`: number of pheaders (incompatible with the range and field selectors)
//! - `pheader.pathname_offset`: reference pathname (mixable with range and other selectors)
//! - `pheader.ref_offset`: offset into the file
//! - `pheader.virtual_range`: virtual address range of the pheader (mixable with range and other selectors)
//! - `pheader.virtual_size`: size of the virtual address range (mixable with range and other selectors)
//! - `pheader.prot`: area `rwx` protections (mixable with range and other selectors)
//! - `pheader.itree`: show the interval tree offset and size in number of nodes (mixable with range and other selectors)
//! - `pheader.zero_pages`: number of zero pages

use jif::*;

mod selectors;
mod utils;

use crate::selectors::*;
use crate::utils::IndexRange;

use std::collections::HashSet;
use std::fs::File;
use std::io::BufReader;

use anyhow::Context;
use clap::Parser;

use self::itree::interval::DataSource;

#[derive(Parser)]
#[command(version)]
/// readjif: read and query JIF files
///
/// This tool parses the JIF (optionally materializing it) and allows for querying and viewing the
/// JIF
struct Cli {
    /// JIF file to read from
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    jif_file: std::path::PathBuf,

    /// Selector command
    ///
    /// For help, type `help` as the subcommand
    command: Option<String>,

    /// Use the raw JIF
    #[arg(short, long)]
    raw: bool,

    /// Just check
    #[arg(short, long)]
    check: bool,
}

fn select_raw(jif: JifRaw, cmd: RawCommand) {
    match cmd {
        RawCommand::Jif(j) => match j {
            RawJifCmd::All => println!("{:#x?}", jif),
            RawJifCmd::Metadata => println!("metadata section: {:#x} B", jif.data_offset()),
            RawJifCmd::Data => println!("data section: {:#x} B", jif.data_size()),
        },
        RawCommand::Strings => {
            for s in jif.strings().iter() {
                println!("{}", s);
            }
        }
        RawCommand::Ord(o) => {
            let ords = jif.ord_chunks();
            match o {
                OrdCmd::All | OrdCmd::Range(IndexRange::None) => println!("{:x?}", ords),
                OrdCmd::Len => println!("ord_len: {}", ords.len()),
                OrdCmd::Vmas => panic!("cannot get vmas for an ord chunk in a raw JIF"),
                OrdCmd::Files => panic!("cannot get files for an ord chunk in a raw JIF"),
                OrdCmd::Range(IndexRange::RightOpen { start }) => println!(
                    "{:x?}",
                    if start < ords.len() {
                        &ords[start..]
                    } else {
                        &[]
                    }
                ),
                OrdCmd::Range(IndexRange::LeftOpen { end }) => {
                    println!("{:x?}", &ords[..std::cmp::min(end, ords.len())])
                }
                OrdCmd::Range(IndexRange::Closed { start, end }) => println!(
                    "{:x?}",
                    if start < ords.len() {
                        &ords[start..std::cmp::min(end, ords.len())]
                    } else {
                        &[]
                    }
                ),
                OrdCmd::Range(IndexRange::Index(idx)) => {
                    if idx < ords.len() {
                        println!("{:x?}", &ords[idx]);
                    }
                }
                OrdCmd::Pages(s) => {
                    print!("{{ ");
                    if s.zero {
                        print!(
                            "private_pages: {}, ",
                            ords.iter()
                                .filter(|o| o.kind() == DataSource::Private)
                                .map(|o| o.size())
                                .sum::<u64>()
                        )
                    }
                    if s.private {
                        print!(
                            "shared_pages: {}, ",
                            ords.iter()
                                .filter(|o| o.kind() == DataSource::Shared)
                                .map(|o| o.size())
                                .sum::<u64>()
                        )
                    }
                    if s.shared {
                        print!(
                            "zero_pages: {}, ",
                            ords.iter()
                                .filter(|o| o.kind() == DataSource::Zero)
                                .map(|o| o.size())
                                .sum::<u64>()
                        )
                    }
                    if s.total {
                        print!("pages: {}, ", ords.iter().map(|o| o.size()).sum::<u64>())
                    }
                    println!("}}");
                }
                OrdCmd::Intervals(_) => {
                    panic!("error: cannot select ord intervals on raw jif");
                }
            }
        }
        RawCommand::ITree(i) => {
            let itree_nodes = jif.itree_nodes();
            match i {
                ITreeCmd::All | ITreeCmd::Range(IndexRange::None) => {
                    println!("{:#x?}", itree_nodes)
                }
                ITreeCmd::Len => println!("n_itree_nodes: {}", itree_nodes.len()),
                ITreeCmd::Range(IndexRange::RightOpen { start }) => println!(
                    "{:#x?}",
                    if start < itree_nodes.len() {
                        &itree_nodes[start..]
                    } else {
                        &[]
                    }
                ),
                ITreeCmd::Range(IndexRange::LeftOpen { end }) => {
                    println!(
                        "{:#x?}",
                        &itree_nodes[..std::cmp::min(end, itree_nodes.len())]
                    )
                }
                ITreeCmd::Range(IndexRange::Closed { start, end }) => println!(
                    "{:#x?}",
                    if start < itree_nodes.len() {
                        &itree_nodes[start..std::cmp::min(end, itree_nodes.len())]
                    } else {
                        &[]
                    }
                ),
                ITreeCmd::Range(IndexRange::Index(idx)) => {
                    if idx < itree_nodes.len() {
                        println!("{:#x?}", itree_nodes[idx]);
                    }
                }
            }
        }
        RawCommand::Pheader(p) => {
            let pheaders = jif.pheaders();
            match p {
                RawPheaderCmd::Len => println!("n_pheaders: {}", pheaders.len()),
                RawPheaderCmd::All => println!("{:#x?}", pheaders),
                RawPheaderCmd::Selector { range, selector } => {
                    let ranged_pheaders = match range {
                        IndexRange::None => pheaders,
                        IndexRange::Closed { start, end } => {
                            if start < pheaders.len() {
                                &pheaders[start..std::cmp::min(end, pheaders.len())]
                            } else {
                                &[]
                            }
                        }
                        IndexRange::LeftOpen { end } => {
                            &pheaders[..std::cmp::min(end, pheaders.len())]
                        }
                        IndexRange::RightOpen { start } => {
                            if start < pheaders.len() {
                                &pheaders[start..]
                            } else {
                                &[]
                            }
                        }
                        IndexRange::Index(idx) => {
                            if idx < pheaders.len() {
                                &pheaders[idx..(idx + 1)]
                            } else {
                                &[]
                            }
                        }
                    };

                    println!("[");
                    for pheader in ranged_pheaders {
                        print!("phdr {{ ");
                        if selector.virtual_range {
                            let (start, end) = pheader.virtual_range();
                            print!("virtual_range: [{:#x}; {:#x}), ", start, end);
                        }
                        if selector.virtual_size {
                            let (start, end) = pheader.virtual_range();
                            print!("virtual_size: {:#x} B, ", end - start);
                        }
                        if selector.pathname_offset {
                            if let Some(offset) = pheader.pathname_offset() {
                                print!("pathname_offset: {:#x}, ", offset);
                            }
                        }
                        if selector.ref_offset {
                            if let Some(offset) = pheader.ref_offset() {
                                print!("ref_offset: {:#x}, ", offset);
                            }
                        }
                        if selector.prot {
                            let prot = pheader.prot();
                            print!(
                                "prot: {}{}{}, ",
                                if prot & Prot::Read as u8 != 0 {
                                    "r"
                                } else {
                                    "-"
                                },
                                if prot & Prot::Write as u8 != 0 {
                                    "w"
                                } else {
                                    "-"
                                },
                                if prot & Prot::Exec as u8 != 0 {
                                    "x"
                                } else {
                                    "-"
                                },
                            )
                        }
                        if selector.itree {
                            if let Some((idx, n_nodes)) = pheader.itree() {
                                print!("itree: [{}; #{}), ", idx, n_nodes);
                            }
                        }
                        println!("}}")
                    }
                    println!("]");
                }
            }
        }
    }
}

fn select_materialized(jif: Jif, cmd: MaterializedCommand) {
    match cmd {
        MaterializedCommand::Jif(j) => match j {
            JifCmd::All => println!("{:#x?}", jif),
            JifCmd::Strings => {
                for s in jif.strings().iter() {
                    println!("{}", s);
                }
            }
            JifCmd::Pages(s) => {
                print!("{{ ");
                if s.zero {
                    print!("zero_pages: {}, ", jif.zero_pages())
                }
                if s.private {
                    print!("private_pages: {}, ", jif.private_pages())
                }
                if s.shared {
                    print!("shared_pages: {}, ", jif.shared_pages())
                }
                if s.total {
                    print!("total_pages: {}, ", jif.total_pages())
                }
                println!("}}");
            }
            JifCmd::Intervals(s) => {
                print!("{{ ");
                if s.zero {
                    print!("zero_intervals: {}, ", jif.n_zero_intervals())
                }
                if s.private {
                    print!("private_intervals: {}, ", jif.n_private_intervals())
                }
                if s.shared {
                    print!("shared_intervals: {}, ", jif.n_shared_intervals())
                }
                if s.total {
                    print!("total_intervals: {}, ", jif.n_intervals())
                }
                println!("}}");
            }
        },
        MaterializedCommand::Ord(o) => {
            let ords = jif.ord_chunks();
            match o {
                OrdCmd::All | OrdCmd::Range(IndexRange::None) => println!("{:#x?}", ords),
                OrdCmd::Len => println!("ord_len: {}", ords.len()),
                OrdCmd::Vmas => println!(
                    "{}",
                    ords.iter()
                        .filter_map(|o| jif.ord_vma(o))
                        .collect::<HashSet<_>>()
                        .len()
                ),
                OrdCmd::Files => panic!("cannot get files for an ord chunk in a raw JIF"),
                OrdCmd::Range(IndexRange::RightOpen { start }) => println!(
                    "{:#x?}",
                    if start < ords.len() {
                        &ords[start..]
                    } else {
                        &[]
                    }
                ),
                OrdCmd::Range(IndexRange::LeftOpen { end }) => {
                    println!("{:#x?}", &ords[..std::cmp::min(end, ords.len())])
                }
                OrdCmd::Range(IndexRange::Closed { start, end }) => println!(
                    "{:#x?}",
                    if start < ords.len() {
                        &ords[start..std::cmp::min(end, ords.len())]
                    } else {
                        &[]
                    }
                ),
                OrdCmd::Range(IndexRange::Index(idx)) => {
                    if idx < ords.len() {
                        println!("{:#x?}", &ords[idx]);
                    }
                }
                OrdCmd::Pages(s) => {
                    print!("{{ ");
                    if s.zero {
                        print!(
                            "private_pages: {}, ",
                            ords.iter()
                                .filter(|o| o.kind() == DataSource::Private)
                                .map(|o| o.size())
                                .sum::<u64>()
                        )
                    }
                    if s.private {
                        print!(
                            "shared_pages: {}, ",
                            ords.iter()
                                .filter(|o| o.kind() == DataSource::Shared)
                                .map(|o| o.size())
                                .sum::<u64>()
                        )
                    }
                    if s.shared {
                        print!(
                            "zero_pages: {}, ",
                            ords.iter()
                                .filter(|o| o.kind() == DataSource::Zero)
                                .map(|o| o.size())
                                .sum::<u64>()
                        )
                    }
                    if s.total {
                        print!("pages: {}, ", ords.iter().map(|o| o.size()).sum::<u64>())
                    }
                    println!("}}");
                }
                OrdCmd::Intervals(s) => {
                    let mut total_intervals = HashSet::new();
                    let mut private_intervals = HashSet::new();
                    let mut shared_intervals = HashSet::new();
                    let mut zero_intervals = HashSet::new();
                    // ASSUMPTION: each ord chunk is in a single interval
                    for o in ords {
                        match jif.resolve(o.addr()) {
                            Some(i) if i.source == DataSource::Private => {
                                total_intervals.insert(i.start);
                                private_intervals.insert(i.start);
                            }
                            Some(i) if i.source == DataSource::Shared => {
                                total_intervals.insert(i.start);
                                shared_intervals.insert(i.start);
                            }
                            Some(i) => {
                                assert!(i.source == DataSource::Zero);
                                total_intervals.insert(i.start);
                                zero_intervals.insert(i.start);
                            }
                            None => {
                                eprintln!("WARN: ordering segment {o:x?} is not mapped by the jif");
                            }
                        }
                    }
                    assert_eq!(
                        total_intervals.len(),
                        private_intervals.len() + shared_intervals.len() + zero_intervals.len()
                    );
                    print!("{{ ");
                    if s.zero {
                        print!("private_intervals: {}, ", zero_intervals.len(),)
                    }
                    if s.private {
                        print!("shared_intervals: {}, ", private_intervals.len(),)
                    }
                    if s.shared {
                        print!("zero_intervals: {}, ", shared_intervals.len(),)
                    }
                    if s.total {
                        print!("intervals: {}, ", total_intervals.len());
                    }
                    println!("}}");
                }
            }
        }
        MaterializedCommand::Pheader(p) => {
            let pheaders = jif.pheaders();
            match p {
                PheaderCmd::Len => println!("n_pheaders: {}", pheaders.len()),
                PheaderCmd::All => println!("{:#x?}", pheaders),
                PheaderCmd::Selector { range, selector } => {
                    let ranged_pheaders = match range {
                        IndexRange::None => pheaders,
                        IndexRange::Closed { start, end } => {
                            if start < pheaders.len() {
                                &pheaders[start..std::cmp::min(end, pheaders.len())]
                            } else {
                                &[]
                            }
                        }
                        IndexRange::LeftOpen { end } => {
                            &pheaders[..std::cmp::min(end, pheaders.len())]
                        }
                        IndexRange::RightOpen { start } => {
                            if start < pheaders.len() {
                                &pheaders[start..]
                            } else {
                                &[]
                            }
                        }
                        IndexRange::Index(idx) => {
                            if idx < pheaders.len() {
                                &pheaders[idx..(idx + 1)]
                            } else {
                                &[]
                            }
                        }
                    };

                    println!("[");
                    for pheader in ranged_pheaders {
                        print!("phdr {{ ");
                        if selector.virtual_range {
                            let (start, end) = pheader.virtual_range();
                            print!("virtual_range: [{:#x}; {:#x}), ", start, end);
                        }
                        if selector.virtual_size {
                            let (start, end) = pheader.virtual_range();
                            print!("virtual_size: {:#x} B, ", end - start);
                        }
                        if selector.data_size {
                            print!("data: {:#x} B, ", pheader.data_size());
                        }
                        if selector.pathname {
                            if let Some(s) = pheader.pathname() {
                                print!("path: {}, ", s);
                            }
                        }
                        if selector.ref_offset {
                            if let Some(offset) = pheader.ref_offset() {
                                print!("ref_offset: {:#x}, ", offset);
                            }
                        }

                        if selector.prot {
                            let prot = pheader.prot();
                            print!(
                                "prot: {}{}{}, ",
                                if prot & Prot::Read as u8 != 0 {
                                    "r"
                                } else {
                                    "-"
                                },
                                if prot & Prot::Write as u8 != 0 {
                                    "w"
                                } else {
                                    "-"
                                },
                                if prot & Prot::Exec as u8 != 0 {
                                    "x"
                                } else {
                                    "-"
                                },
                            )
                        }
                        if selector.itree {
                            print!("itree: {:?}, ", pheader.itree());
                        }
                        if selector.n_itree_nodes {
                            print!("n_itree_nodes: {:?}, ", pheader.n_itree_nodes());
                        }
                        if selector.zero_pages {
                            print!("zero_pages: {}, ", pheader.zero_pages())
                        }
                        if selector.private_pages {
                            print!("private_pages: {}, ", pheader.private_pages())
                        }
                        if selector.shared_pages {
                            print!("shared_pages: {}, ", pheader.shared_pages())
                        }
                        if selector.pages {
                            print!("total_pages: {}, ", pheader.total_pages())
                        }
                        println!("}}")
                    }
                    println!("]");
                }
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    if args.check {
        let mut file = BufReader::new(File::open(&args.jif_file).context("failed to open file")?);
        if args.raw {
            JifRaw::from_reader(&mut file).context("failed to open jif in raw mode")?;
        } else {
            Jif::from_reader(&mut file).context("failed to open jif in raw mode")?;
        }
        return Ok(());
    }

    if args.raw {
        let cmd: RawCommand = args.command.try_into().map_err(|e| {
            anyhow::anyhow!(
                "failed to parse raw selector command: {}\n{}",
                e,
                RAW_COMMAND_USAGE,
            )
        })?;

        let mut file = BufReader::new(File::open(&args.jif_file).context("failed to open file")?);
        let jif = JifRaw::from_reader(&mut file).context("failed to open jif in raw mode")?;
        select_raw(jif, cmd)
    } else {
        let cmd: MaterializedCommand = args.command.try_into().map_err(|e| {
            anyhow::anyhow!(
                "failed to parse materialized selector command: {}\n{}",
                e,
                MATERIALIZED_COMMAND_USAGE
            )
        })?;

        let mut file = BufReader::new(File::open(&args.jif_file).context("failed to open file")?);
        let jif = Jif::from_reader(&mut file).context("failed to open jif")?;
        select_materialized(jif, cmd)
    }

    Ok(())
}
