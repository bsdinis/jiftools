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
use json::JsonValue;

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

    /// Selector commands
    ///
    /// For help, type `help` as the subcommand
    commands: Vec<String>,

    /// Use the raw JIF
    #[arg(short, long)]
    raw: bool,

    /// Just check
    #[arg(short, long)]
    check: bool,
}

fn select_raw(jif: JifRaw, cmds: Vec<RawCommand>) -> Result<JsonValue, json::Error> {
    let mut json_value = JsonValue::new_object();
    for cmd in cmds {
        match cmd {
            RawCommand::Jif(j) => match j {
                RawJifCmd::All => {
                    json_value.insert("jif", format!("{jif:#x?}"))?;
                }
                RawJifCmd::Metadata => {
                    json_value
                        .insert("jif.metadata_size", format!("{:#x?} B", jif.data_offset()))?;
                }
                RawJifCmd::Data => {
                    json_value.insert("jif.data_size", format!("{:#x?} B", jif.data_size()))?;
                }
            },
            RawCommand::Strings => {
                json_value.insert("jif.strings", jif.strings())?;
            }
            RawCommand::Ord(o) => {
                let ords = jif.ord_chunks();
                match o {
                    OrdCmd::All | OrdCmd::Range(IndexRange::None) => {
                        json_value.insert("ord", format!("{ords:#x?}"))?;
                    }
                    OrdCmd::Len => {
                        json_value.insert("ord.len", ords.len())?;
                    }
                    OrdCmd::Vmas => panic!("cannot get vmas for an ord chunk in a raw JIF"),
                    OrdCmd::Files => panic!("cannot get files for an ord chunk in a raw JIF"),
                    OrdCmd::Range(IndexRange::RightOpen { start }) => {
                        json_value.insert(
                            "ord",
                            format!(
                                "{:x?}",
                                if start < ords.len() {
                                    &ords[start..]
                                } else {
                                    &[]
                                }
                            ),
                        )?;
                    }
                    OrdCmd::Range(IndexRange::LeftOpen { end }) => {
                        json_value.insert(
                            "ord",
                            format!("{:x?}", &ords[..std::cmp::min(end, ords.len())]),
                        )?;
                    }
                    OrdCmd::Range(IndexRange::Closed { start, end }) => {
                        json_value.insert(
                            "ord",
                            format!(
                                "{:x?}",
                                if start < ords.len() {
                                    &ords[start..std::cmp::min(end, ords.len())]
                                } else {
                                    &[]
                                }
                            ),
                        )?;
                    }
                    OrdCmd::Range(IndexRange::Index(idx)) => {
                        if idx < ords.len() {
                            json_value.insert("ord", format!("{:x?}", &ords[idx]))?;
                        }
                    }
                    OrdCmd::Pages(s) => {
                        if s.private {
                            json_value.insert(
                                "ord.private_pages",
                                ords.iter()
                                    .filter(|o| o.kind() == DataSource::Private)
                                    .map(|o| o.size())
                                    .sum::<u64>(),
                            )?;
                        }
                        if s.shared {
                            json_value.insert(
                                "ord.shared_pages",
                                ords.iter()
                                    .filter(|o| o.kind() == DataSource::Shared)
                                    .map(|o| o.size())
                                    .sum::<u64>(),
                            )?;
                        }
                        if s.zero {
                            json_value.insert(
                                "ord.zero",
                                ords.iter()
                                    .filter(|o| o.kind() == DataSource::Zero)
                                    .map(|o| o.size())
                                    .sum::<u64>(),
                            )?;
                        }
                        if s.total {
                            json_value
                                .insert("ord.pages", ords.iter().map(|o| o.size()).sum::<u64>())?;
                        }
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
                        json_value.insert("itree", format!("{:#x?}", itree_nodes))?;
                    }
                    ITreeCmd::Len => {
                        json_value.insert("itree.len", itree_nodes.len())?;
                    }
                    ITreeCmd::Range(IndexRange::RightOpen { start }) => {
                        json_value.insert(
                            "itree",
                            format!(
                                "{:#x?}",
                                if start < itree_nodes.len() {
                                    &itree_nodes[start..]
                                } else {
                                    &[]
                                }
                            ),
                        )?;
                    }
                    ITreeCmd::Range(IndexRange::LeftOpen { end }) => {
                        json_value.insert(
                            "itree",
                            format!(
                                "{:#x?}",
                                &itree_nodes[..std::cmp::min(end, itree_nodes.len())]
                            ),
                        )?;
                    }
                    ITreeCmd::Range(IndexRange::Closed { start, end }) => {
                        json_value.insert(
                            "itree",
                            format!(
                                "{:#x?}",
                                if start < itree_nodes.len() {
                                    &itree_nodes[start..std::cmp::min(end, itree_nodes.len())]
                                } else {
                                    &[]
                                }
                            ),
                        )?;
                    }
                    ITreeCmd::Range(IndexRange::Index(idx)) => {
                        if idx < itree_nodes.len() {
                            json_value.insert("itree", format!("{:#x?}", itree_nodes[idx]))?;
                        }
                    }
                }
            }
            RawCommand::Pheader(p) => {
                let pheaders = jif.pheaders();
                match p {
                    RawPheaderCmd::Len => {
                        json_value.insert("pheader.len", pheaders.len())?;
                    }
                    RawPheaderCmd::All => {
                        json_value.insert("pheader", format!("{:#x?}", pheaders))?;
                    }
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

                        let mut pheaders = JsonValue::new_array();
                        for pheader in ranged_pheaders {
                            let mut pheader_json = JsonValue::new_object();
                            if selector.virtual_range {
                                let (start, end) = pheader.virtual_range();
                                pheader_json.insert(
                                    "virtual_range",
                                    format!("[{:#x}; {:#x})", start, end),
                                )?;
                            }
                            if selector.virtual_size {
                                let (start, end) = pheader.virtual_range();
                                pheader_json
                                    .insert("virtual_size", format!("{:#x} B", end - start))?;
                            }
                            if selector.pathname_offset {
                                if let Some(offset) = pheader.pathname_offset() {
                                    pheader_json.insert("pathname_offset", offset)?;
                                }
                            }
                            if selector.ref_offset {
                                if let Some(offset) = pheader.ref_offset() {
                                    pheader_json.insert("ref_offset", offset)?;
                                }
                            }
                            if selector.prot {
                                let prot = pheader.prot();
                                pheader_json.insert(
                                    "prot",
                                    format!(
                                        "{}{}{}",
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
                                    ),
                                )?;
                            }
                            if selector.itree {
                                if let Some((idx, n_nodes)) = pheader.itree() {
                                    pheader_json
                                        .insert("itree", format!("[{}; #{}), ", idx, n_nodes))?;
                                }
                            }
                            pheaders.push(pheader_json)?;
                        }
                        json_value.insert("pheaders", pheaders)?;
                    }
                }
            }
        }
    }

    Ok(json_value)
}

fn select_materialized(jif: Jif, cmds: Vec<MaterializedCommand>) -> json::Result<JsonValue> {
    let mut json_value = JsonValue::new_object();
    for cmd in cmds {
        match cmd {
            MaterializedCommand::Jif(j) => match j {
                JifCmd::All => {
                    json_value.insert("jif", format!("{jif:#x?}"))?;
                }
                JifCmd::Strings => {
                    let mut strings = JsonValue::new_array();
                    jif.strings()
                        .iter()
                        .map(|s| strings.push(*s))
                        .collect::<Result<(), _>>()?;
                    json_value.insert("jif.strings", strings)?;
                }
                JifCmd::Pages(s) => {
                    if s.zero {
                        json_value.insert("jif.zero_pages", jif.zero_pages())?;
                    }
                    if s.private {
                        json_value.insert("jif.private_pages", jif.private_pages())?;
                    }
                    if s.shared {
                        json_value.insert("jif.shared_pages", jif.shared_pages())?;
                    }
                    if s.total {
                        json_value.insert("jif.pages", jif.total_pages())?;
                    }
                }
                JifCmd::Intervals(s) => {
                    if s.zero {
                        json_value.insert("jif.zero_intervals", jif.n_zero_intervals())?;
                    }
                    if s.private {
                        json_value.insert("jif.private_intervals", jif.n_private_intervals())?;
                    }
                    if s.shared {
                        json_value.insert("jif.shared_intervals", jif.n_shared_intervals())?;
                    }
                    if s.total {
                        json_value.insert("jif.intervals", jif.n_intervals())?;
                    }
                }
            },
            MaterializedCommand::Ord(o) => {
                let ords = jif.ord_chunks();
                match o {
                    OrdCmd::All | OrdCmd::Range(IndexRange::None) => {
                        json_value.insert("ord", format!("{ords:#x?}"))?;
                    }
                    OrdCmd::Len => {
                        json_value.insert("ord.len", ords.len())?;
                    }
                    OrdCmd::Vmas => {
                        json_value.insert(
                            "ord.vmas",
                            format!(
                                "{}",
                                ords.iter()
                                    .filter_map(|o| jif.ord_vma(o))
                                    .collect::<HashSet<_>>()
                                    .len()
                            ),
                        )?;
                    }
                    OrdCmd::Files => panic!("cannot get files for an ord chunk in a raw JIF"),
                    OrdCmd::Range(IndexRange::RightOpen { start }) => {
                        json_value.insert(
                            "ord",
                            format!(
                                "{:x?}",
                                if start < ords.len() {
                                    &ords[start..]
                                } else {
                                    &[]
                                }
                            ),
                        )?;
                    }
                    OrdCmd::Range(IndexRange::LeftOpen { end }) => {
                        json_value.insert(
                            "ord",
                            format!("{:x?}", &ords[..std::cmp::min(end, ords.len())]),
                        )?;
                    }
                    OrdCmd::Range(IndexRange::Closed { start, end }) => {
                        json_value.insert(
                            "ord",
                            format!(
                                "{:x?}",
                                if start < ords.len() {
                                    &ords[start..std::cmp::min(end, ords.len())]
                                } else {
                                    &[]
                                }
                            ),
                        )?;
                    }
                    OrdCmd::Range(IndexRange::Index(idx)) => {
                        if idx < ords.len() {
                            json_value.insert("ord", format!("{:x?}", &ords[idx]))?;
                        }
                    }
                    OrdCmd::Pages(s) => {
                        if s.private {
                            json_value.insert(
                                "ord.private_pages",
                                ords.iter()
                                    .filter(|o| o.kind() == DataSource::Private)
                                    .map(|o| o.size())
                                    .sum::<u64>(),
                            )?;
                        }
                        if s.shared {
                            json_value.insert(
                                "ord.shared_pages",
                                ords.iter()
                                    .filter(|o| o.kind() == DataSource::Shared)
                                    .map(|o| o.size())
                                    .sum::<u64>(),
                            )?;
                        }
                        if s.zero {
                            json_value.insert(
                                "ord.zero_pages",
                                ords.iter()
                                    .filter(|o| o.kind() == DataSource::Zero)
                                    .map(|o| o.size())
                                    .sum::<u64>(),
                            )?;
                        }
                        if s.total {
                            json_value
                                .insert("ord.pages", ords.iter().map(|o| o.size()).sum::<u64>())?;
                        }
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
                                    eprintln!(
                                        "WARN: ordering segment {o:x?} is not mapped by the jif"
                                    );
                                }
                            }
                        }
                        assert_eq!(
                            total_intervals.len(),
                            private_intervals.len() + shared_intervals.len() + zero_intervals.len()
                        );
                        if s.private {
                            json_value.insert("ord.private_intervals", private_intervals.len())?;
                        }
                        if s.shared {
                            json_value.insert("ord.shared_intervals", shared_intervals.len())?;
                        }
                        if s.zero {
                            json_value.insert("ord.zero_intervals", zero_intervals.len())?;
                        }
                        if s.total {
                            json_value.insert("ord.intervals", total_intervals.len())?;
                        }
                    }
                }
            }
            MaterializedCommand::Pheader(p) => {
                let pheaders = jif.pheaders();
                match p {
                    PheaderCmd::Len => {
                        json_value.insert("pheader.len", pheaders.len())?;
                    }
                    PheaderCmd::All => {
                        json_value.insert("pheader", format!("{:#x?}", pheaders))?;
                    }
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

                        let mut pheaders = JsonValue::new_array();
                        for pheader in ranged_pheaders {
                            let mut pheader_json = JsonValue::new_object();
                            if selector.virtual_range {
                                let (start, end) = pheader.virtual_range();
                                pheader_json.insert(
                                    "virtual_range",
                                    format!("[{:#x}; {:#x})", start, end),
                                )?;
                            }
                            if selector.virtual_size {
                                let (start, end) = pheader.virtual_range();
                                pheader_json
                                    .insert("virtual_size", format!("{:#x} B", end - start))?;
                            }
                            if selector.data_size {
                                pheader_json
                                    .insert("data_size", format!("{:#x} B", pheader.data_size()))?;
                            }
                            if selector.pathname {
                                if let Some(s) = pheader.pathname() {
                                    pheader_json.insert("path", s)?;
                                }
                            }
                            if selector.ref_offset {
                                if let Some(offset) = pheader.ref_offset() {
                                    pheader_json.insert("ref_offset", offset)?;
                                }
                            }

                            if selector.prot {
                                let prot = pheader.prot();
                                pheader_json.insert(
                                    "prot",
                                    format!(
                                        "{}{}{}",
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
                                    ),
                                )?;
                            }
                            if selector.itree {
                                pheader_json.insert("itree", format!("{:?}, ", pheader.itree()))?;
                            }
                            if selector.n_itree_nodes {
                                pheader_json.insert("itree.len", pheader.n_itree_nodes())?;
                            }
                            if selector.zero_pages {
                                pheader_json.insert("zero_pages", pheader.zero_pages())?;
                            }
                            if selector.private_pages {
                                pheader_json.insert("private_pages", pheader.private_pages())?;
                            }
                            if selector.shared_pages {
                                pheader_json.insert("shared_pages", pheader.shared_pages())?;
                            }
                            if selector.pages {
                                pheader_json.insert("pages", pheader.total_pages())?;
                            }
                            pheaders.push(pheader_json)?;
                        }
                        json_value.insert("pheaders", pheaders)?;
                    }
                }
            }
        }
    }

    Ok(json_value)
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    let mut file = BufReader::new(
        File::open(&args.jif_file)
            .with_context(|| format!("failed to open file: {}", args.jif_file.display()))?,
    );

    if args.check {
        if args.raw {
            JifRaw::from_reader(&mut file).context("failed to open jif in raw mode")?;
        } else {
            Jif::from_reader(&mut file).context("failed to open jif in raw mode")?;
        }
        return Ok(());
    }

    let json_value = if args.raw {
        let cmds: Vec<RawCommand> = args
            .commands
            .into_iter()
            .map(|x| x.try_into())
            .collect::<Result<_, _>>()
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to parse raw selector command: {}\n{}",
                    e,
                    RAW_COMMAND_USAGE,
                )
            })?;

        let cmds = if cmds.is_empty() {
            vec![RawCommand::Jif(RawJifCmd::All)]
        } else {
            cmds
        };

        let jif = JifRaw::from_reader(&mut file).context("failed to open jif in raw mode")?;
        select_raw(jif, cmds).map_err(|e| anyhow::anyhow!("json error: {e:?}"))
    } else {
        let cmds: Vec<MaterializedCommand> = args
            .commands
            .into_iter()
            .map(|x| x.try_into())
            .collect::<Result<_, _>>()
            .map_err(|e| {
                anyhow::anyhow!(
                    "failed to parse materialized selector command: {}\n{}",
                    e,
                    MATERIALIZED_COMMAND_USAGE,
                )
            })?;

        let cmds = if cmds.is_empty() {
            vec![MaterializedCommand::Jif(JifCmd::All)]
        } else {
            cmds
        };

        let jif = Jif::from_reader(&mut file).context("failed to open jif")?;
        select_materialized(jif, cmds).map_err(|e| anyhow::anyhow!("json error: {e:?}"))
    }?;

    print!("{}", json_value.pretty(4));
    Ok(())
}
