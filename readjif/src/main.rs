use jif::*;

mod selectors;
mod utils;

use crate::selectors::*;

use std::fs::File;
use std::io::BufReader;

use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(value_name = "FILE", value_hint = clap::ValueHint::FilePath)]
    jif_file: std::path::PathBuf,

    #[arg(long)]
    raw: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    Map { cmd: String },
}

fn select_raw(jif: JifRaw, cmd: RawCommand) {
    match cmd {
        RawCommand::Jif { data } => {
            if data {
                println!("data section: {:#x} B", jif.data().len())
            } else {
                println!("{:#x?}", jif)
            }
        }
        RawCommand::Strings => println!("{:#x?}", jif.strings()),
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
                        print!("phrd {{ ");
                        if selector.virtual_range {
                            let (start, end) = pheader.virtual_range();
                            print!("virtual_range: [{:#x}; {:#x}), ", start, end);
                        }
                        if selector.data {
                            let (start, end) = pheader.data_range();
                            print!("data_range: [{:#x}; {:#x}), ", start, end);
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
                        if selector.itree {
                            if let Some(it) = pheader.itree() {
                                print!("itree: {:?}, ", it);
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
        MaterializedCommand::Jif { strings } => {
            if strings {
                println!("{:#x?}", jif.strings())
            } else {
                println!("{:#x?}", jif)
            }
        }
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
                        print!("phrd {{ ");
                        if selector.virtual_range {
                            let (start, end) = pheader.virtual_range();
                            print!("virtual_range: [{:#x}; {:#x}), ", start, end);
                        }
                        if selector.data {
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
                        if selector.itree {
                            if let Some(it) = pheader.itree() {
                                print!("itree: {:?}, ", it);
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
        let cmd: RawCommand = args.command.try_into()?;
        select_raw(jif, cmd)
    } else {
        let jif = Jif::from_reader(&mut file).context("failed to open jif")?;
        let cmd: MaterializedCommand = args.command.try_into()?;
        select_materialized(jif, cmd)
    }

    Ok(())
}
