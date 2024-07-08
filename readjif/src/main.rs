use jif::*;

mod selectors;
mod utils;

use crate::selectors::*;

use std::fs::File;
use std::io::BufReader;

use anyhow::Context;
use clap::Parser;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    jif_file: std::path::PathBuf,

    command: Option<String>,

    #[arg(long)]
    raw: bool,
}

fn select_raw(jif: JifRaw, cmd: RawCommand) {
    match cmd {
        RawCommand::Jif(j) => match j {
            RawJifCmd::All => println!("{:#x?}", jif),
            RawJifCmd::Data => println!("data section: {:#x} B", jif.data().len()),
        },
        RawCommand::Strings => {
            for s in jif.strings().iter() {
                println!("{}", s);
            }
        }
        RawCommand::Ord(o) => {
            let ords = jif.ord_chunks();
            match o {
                OrdCmd::All => println!("{:#x?}", ords),
                OrdCmd::Len => println!("ord_len: {}", ords.len()),
                OrdCmd::Range(None, None) => println!("{:#x?}", ords),
                OrdCmd::Range(Some(start), None) => println!(
                    "{:#x?}",
                    if start < ords.len() {
                        &ords[start..]
                    } else {
                        &[]
                    }
                ),
                OrdCmd::Range(None, Some(end)) => {
                    println!("{:#x?}", &ords[..std::cmp::min(end, ords.len())])
                }
                OrdCmd::Range(Some(start), Some(end)) => println!(
                    "{:#x?}",
                    if start < ords.len() {
                        &ords[start..std::cmp::min(end, ords.len())]
                    } else {
                        &[]
                    }
                ),
            }
        }
        RawCommand::ITree(i) => {
            let itree_nodes = jif.itree_nodes();
            match i {
                ITreeCmd::All => println!("{:#x?}", itree_nodes),
                ITreeCmd::Len => println!("n_itree_nodes: {}", itree_nodes.len()),
                ITreeCmd::Range(None, None) => println!("{:#x?}", itree_nodes),
                ITreeCmd::Range(Some(start), None) => println!(
                    "{:#x?}",
                    if start < itree_nodes.len() {
                        &itree_nodes[start..]
                    } else {
                        &[]
                    }
                ),
                ITreeCmd::Range(None, Some(end)) => {
                    println!(
                        "{:#x?}",
                        &itree_nodes[..std::cmp::min(end, itree_nodes.len())]
                    )
                }
                ITreeCmd::Range(Some(start), Some(end)) => println!(
                    "{:#x?}",
                    if start < itree_nodes.len() {
                        &itree_nodes[start..std::cmp::min(end, itree_nodes.len())]
                    } else {
                        &[]
                    }
                ),
            }
        }
        RawCommand::Pheader(p) => {
            let pheaders = jif.pheaders();
            match p {
                RawPheaderCmd::Len => println!("n_pheaders: {}", pheaders.len()),
                RawPheaderCmd::All => println!("{:#x?}", pheaders),
                RawPheaderCmd::Selector { range, selector } => {
                    let ranged_pheaders = match range {
                        None => pheaders,
                        Some((Some(start), Some(end))) => {
                            if start < pheaders.len() {
                                &pheaders[start..std::cmp::min(end, pheaders.len())]
                            } else {
                                &[]
                            }
                        }
                        Some((None, Some(end))) => &pheaders[..std::cmp::min(end, pheaders.len())],
                        Some((Some(start), None)) => {
                            if start < pheaders.len() {
                                &pheaders[start..]
                            } else {
                                &[]
                            }
                        }
                        Some((None, None)) => pheaders,
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
                        if selector.data_range {
                            let (start, end) = pheader.data_range();
                            print!("data_range: [{:#x}; {:#x}), ", start, end);
                        }
                        if selector.data_size {
                            let (start, end) = pheader.data_range();
                            print!("data_size: {:#x} B, ", end - start);
                        }
                        if selector.pathname_offset {
                            if let Some(offset) = pheader.pathname_offset() {
                                print!("pathname_offset: {:#x}, ", offset);
                            }
                        }
                        if selector.ref_range {
                            if let Some((start, end)) = pheader.ref_range() {
                                print!("ref_range: [{:#x}; {:#x}), ", start, end);
                            }
                        }
                        if selector.ref_size {
                            if let Some((start, end)) = pheader.ref_range() {
                                print!("ref_size: {:#x} B, ", end - start);
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
            JifCmd::Pages(p) => {
                print!("{{ ");
                if p.zero {
                    print!("zero_pages: {}, ", jif.zero_pages())
                }
                if p.private {
                    print!("private_pages: {}, ", jif.private_pages())
                }
                if p.shared {
                    print!("shared_pages: {}, ", jif.shared_pages())
                }
                if p.total {
                    print!("total_pages: {}, ", jif.total_pages())
                }
                println!("}}");
            }
        },
        MaterializedCommand::Ord(o) => {
            let ords = jif.ord_chunks();
            match o {
                OrdCmd::All => println!("{:#x?}", ords),
                OrdCmd::Len => println!("ord_len: {}", ords.len()),
                OrdCmd::Range(None, None) => println!("{:#x?}", ords),
                OrdCmd::Range(Some(start), None) => println!(
                    "{:#x?}",
                    if start < ords.len() {
                        &ords[start..]
                    } else {
                        &[]
                    }
                ),
                OrdCmd::Range(None, Some(end)) => {
                    println!("{:#x?}", &ords[..std::cmp::min(end, ords.len())])
                }
                OrdCmd::Range(Some(start), Some(end)) => println!(
                    "{:#x?}",
                    if start < ords.len() {
                        &ords[start..std::cmp::min(end, ords.len())]
                    } else {
                        &[]
                    }
                ),
            }
        }
        MaterializedCommand::Pheader(p) => {
            let pheaders = jif.pheaders();
            match p {
                PheaderCmd::Len => println!("n_pheaders: {}", pheaders.len()),
                PheaderCmd::All => println!("{:#x?}", pheaders),
                PheaderCmd::Selector { range, selector } => {
                    let ranged_pheaders = match range {
                        None => pheaders,
                        Some((Some(start), Some(end))) => {
                            if start < pheaders.len() {
                                &pheaders[start..std::cmp::min(end, pheaders.len())]
                            } else {
                                &[]
                            }
                        }
                        Some((None, Some(end))) => &pheaders[..std::cmp::min(end, pheaders.len())],
                        Some((Some(start), None)) => {
                            if start < pheaders.len() {
                                &pheaders[start..]
                            } else {
                                &[]
                            }
                        }
                        Some((None, None)) => pheaders,
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
                            print!("data: {:#x} B, ", pheader.data().len());
                        }
                        if selector.pathname {
                            if let Some(s) = pheader.pathname() {
                                print!("path: {}, ", s);
                            }
                        }
                        if selector.ref_range {
                            if let Some((start, end)) = pheader.ref_range() {
                                print!("ref_range: [{:#x}; {:#x}), ", start, end);
                            }
                        }
                        if selector.ref_size {
                            if let Some((start, end)) = pheader.ref_range() {
                                print!("ref_size: {:#x} B, ", end - start);
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
                            if let Some(it) = pheader.itree() {
                                print!("itree: {:?}, ", it);
                            }
                        }
                        if selector.n_itree_nodes {
                            if let Some(it) = pheader.itree() {
                                print!("n_itree_nodes: {:?}, ", it.n_nodes());
                            }
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
    let mut file = BufReader::new(File::open(&args.jif_file).context("failed to open file")?);

    if args.raw {
        let jif = JifRaw::from_reader(&mut file).context("failed to open jif in raw mode")?;
        let cmd: RawCommand = args.command.try_into().map_err(|e| {
            anyhow::anyhow!(
                "failed to parse raw selector command: {}\n{}",
                e,
                RAW_COMMAND_USAGE,
            )
        })?;
        select_raw(jif, cmd)
    } else {
        let jif = Jif::from_reader(&mut file).context("failed to open jif")?;
        let cmd: MaterializedCommand = args.command.try_into().map_err(|e| {
            anyhow::anyhow!(
                "failed to parse materialized selector command: {}\n{}",
                e,
                MATERIALIZED_COMMAND_USAGE
            )
        })?;
        select_materialized(jif, cmd)
    }

    Ok(())
}
