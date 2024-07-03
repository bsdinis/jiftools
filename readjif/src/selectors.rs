use crate::utils::*;

pub(crate) const MATERIALIZED_COMMAND_USAGE: &str = "materialized command: selection over the materialized JIF representation

jif                                select the whole JIF
jif.strings                        strings in the JIF

ord                                select all the ord chunks
ord[<range>]                       select the ord chunks in the range
ord.len                            number of ord chunks

pheader                            select all the pheaders
pheader[<range>]                   select the pheaders in the range
pheader.len                        number of pheaders
pheader.data                       data range of the pheaders (mixable with range and other selectors)
pheader.path                       reference pathname (mixable with range and other selectors)
pheader.ref_range                  reference range in the path (mixable with range and other selectors)
pheader.virtual_range              virtual address range of the pheader (mixable with range and other selectors)
pheader.prot                       area `rwx` protections (mixable with range and other selectors)
pheader.itree                      pheader interval tree (mixable with range and other selectors)
";

#[derive(Debug)]
pub(crate) enum MaterializedCommand {
    Ord(OrdCmd),
    Pheader(PheaderCmd),
    Jif { strings: bool },
}

#[derive(Debug)]
pub(crate) enum OrdCmd {
    All,
    Range(Option<usize>, Option<usize>),
    Len,
}

#[derive(Debug)]
pub(crate) struct PheaderSelector {
    pub(crate) data: bool,
    pub(crate) pathname: bool,
    pub(crate) ref_range: bool,
    pub(crate) virtual_range: bool,
    pub(crate) prot: bool,
    pub(crate) itree: bool,
}

#[derive(Debug)]
pub(crate) enum PheaderCmd {
    Len,
    Selector {
        range: Option<(Option<usize>, Option<usize>)>,
        selector: PheaderSelector,
    },
    All,
}

pub(crate) const RAW_COMMAND_USAGE: &str = "raw command: selection over the raw JIF representation

jif                                select the whole JIF
jif.data                           size of the data section

strings                            select the strings in the JIF

itrees                             select all the interval trees
itrees[<range>]                    select the interval trees in the range
itrees.len                         number of interval trees

ord                                select all the ord chunks
ord[<range>]                       select the ord chunks in the range
ord.len                            number of ord chunks

pheader                            select all the pheaders
pheader[<range>]                   select the pheaders in the range
pheader.len                        number of pheaders
pheader.data                       data range of the pheaders (mixable with range and other selectors)
pheader.pathname_offset            reference pathname (mixable with range and other selectors)
pheader.ref_range                  reference range in the path (mixable with range and other selectors)
pheader.virtual_range              virtual address range of the pheader (mixable with range and other selectors)
pheader.prot                       area `rwx` protections (mixable with range and other selectors)
pheader.itree                      show the interval tree offset and size in number of nodes (mixable with range and other selectors)
";

#[derive(Debug)]
pub(crate) enum RawCommand {
    Ord(OrdCmd),
    Pheader(RawPheaderCmd),
    Strings,
    ITree(ITreeCmd),
    Jif { data: bool },
}

#[derive(Debug)]
pub(crate) enum ITreeCmd {
    All,
    Range(Option<usize>, Option<usize>),
    Len,
}

#[derive(Debug)]
pub(crate) struct RawPheaderSelector {
    pub(crate) data: bool,
    pub(crate) pathname_offset: bool,
    pub(crate) ref_range: bool,
    pub(crate) virtual_range: bool,
    pub(crate) prot: bool,
    pub(crate) itree: bool,
}

#[derive(Debug)]
pub(crate) enum RawPheaderCmd {
    Len,
    Selector {
        range: Option<(Option<usize>, Option<usize>)>,
        selector: RawPheaderSelector,
    },
    All,
}

impl TryFrom<Option<String>> for MaterializedCommand {
    type Error = anyhow::Error;
    fn try_from(cmd: Option<String>) -> Result<Self, Self::Error> {
        Ok(match cmd {
            Some(cmd) => {
                let trimmed = cmd.trim();

                if trimmed.starts_with("jif") {
                    let (_prefix, suffix) = trimmed.split_at("jif".len());

                    let options = ["", ".strings"];
                    let idx = find_single_option(trimmed, suffix, &options)?;

                    let strings = options[idx] == ".strings";
                    MaterializedCommand::Jif { strings }
                } else if trimmed.starts_with("ord") {
                    let (_prefix, suffix) = trimmed.split_at("ord".len());
                    let (range, suffix) = find_range(trimmed, suffix)?;

                    if let Some((start, end)) = range {
                        if !suffix.is_empty() {
                            return Err(anyhow::anyhow!(
                                "trailing data after range in {}: {}",
                                trimmed,
                                suffix
                            ));
                        }

                        MaterializedCommand::Ord(OrdCmd::Range(start, end))
                    } else {
                        let options = ["", ".len"];
                        let idx = find_single_option(trimmed, suffix, &options)?;
                        if idx == 1 {
                            MaterializedCommand::Ord(OrdCmd::Len)
                        } else {
                            MaterializedCommand::Ord(OrdCmd::All)
                        }
                    }
                } else if trimmed.starts_with("pheader") {
                    let (_prefix, suffix) = trimmed.split_at("pheader".len());
                    let (range, suffix) = find_range(trimmed, suffix)?;

                    let options = [
                        "",               // 0
                        ".len",           // 1
                        ".data",          // 2
                        ".path",          // 3
                        ".ref_range",     // 4
                        ".virtual_range", // 5
                        ".prot",          // 6
                        ".itree",         // 7
                    ];
                    let found_options = find_multiple_option(trimmed, suffix, &options)?;

                    if found_options.contains(&0) {
                        MaterializedCommand::Pheader(PheaderCmd::All)
                    } else if found_options.contains(&1) {
                        if range.is_some() || found_options.len() > 1 {
                            return Err(anyhow::anyhow!(
                                "length option is incompatible with the other options"
                            ));
                        }

                        MaterializedCommand::Pheader(PheaderCmd::Len)
                    } else {
                        let mut selector = PheaderSelector {
                            data: false,
                            pathname: false,
                            ref_range: false,
                            virtual_range: false,
                            prot: false,
                            itree: false,
                        };

                        if found_options.contains(&2) {
                            selector.data = true;
                        }
                        if found_options.contains(&3) {
                            selector.pathname = true;
                        }
                        if found_options.contains(&4) {
                            selector.ref_range = true;
                        }
                        if found_options.contains(&5) {
                            selector.virtual_range = true;
                        }
                        if found_options.contains(&6) {
                            selector.prot = true;
                        }
                        if found_options.contains(&7) {
                            selector.itree = true;
                        }

                        MaterializedCommand::Pheader(PheaderCmd::Selector { range, selector })
                    }
                } else {
                    return Err(anyhow::anyhow!("unknown selector {}", trimmed));
                }
            }
            None => MaterializedCommand::Jif { strings: false },
        })
    }
}

impl TryFrom<Option<String>> for RawCommand {
    type Error = anyhow::Error;
    fn try_from(cmd: Option<String>) -> Result<Self, Self::Error> {
        Ok(match cmd {
            Some(cmd) => {
                let trimmed = cmd.trim();

                if trimmed.starts_with("jif") {
                    let (_prefix, suffix) = trimmed.split_at("jif".len());

                    let options = ["", ".data"];
                    let idx = find_single_option(trimmed, suffix, &options)?;
                    let data = options[idx] == ".data";
                    RawCommand::Jif { data }
                } else if trimmed.starts_with("strings") {
                    let (_prefix, suffix) = trimmed.split_at("strings".len());

                    let options = [""];
                    let _idx = find_single_option(trimmed, suffix, &options)?;
                    RawCommand::Strings
                } else if trimmed.starts_with("ord") {
                    let (_prefix, suffix) = trimmed.split_at("ord".len());
                    let (range, suffix) = find_range(trimmed, suffix)?;

                    if let Some((start, end)) = range {
                        if !suffix.is_empty() {
                            return Err(anyhow::anyhow!(
                                "trailing data after range in {}: {}",
                                trimmed,
                                suffix
                            ));
                        }

                        RawCommand::Ord(OrdCmd::Range(start, end))
                    } else {
                        let options = ["", ".len"];
                        let idx = find_single_option(trimmed, suffix, &options)?;
                        if idx == 1 {
                            RawCommand::Ord(OrdCmd::Len)
                        } else {
                            RawCommand::Ord(OrdCmd::All)
                        }
                    }
                } else if trimmed.starts_with("itree") {
                    let (_prefix, suffix) = trimmed.split_at("itree".len());
                    let (range, suffix) = find_range(trimmed, suffix)?;

                    if let Some((start, end)) = range {
                        if !suffix.is_empty() {
                            return Err(anyhow::anyhow!(
                                "trailing data after range in {}: {}",
                                trimmed,
                                suffix
                            ));
                        }

                        RawCommand::ITree(ITreeCmd::Range(start, end))
                    } else {
                        let options = ["", ".len"];
                        let idx = find_single_option(trimmed, suffix, &options)?;
                        if idx == 1 {
                            RawCommand::ITree(ITreeCmd::Len)
                        } else {
                            RawCommand::ITree(ITreeCmd::All)
                        }
                    }
                } else if trimmed.starts_with("pheader") {
                    let (_prefix, suffix) = trimmed.split_at("pheader".len());
                    let (range, suffix) = find_range(trimmed, suffix)?;

                    let options = [
                        "",                 // 0
                        ".len",             // 1
                        ".data",            // 2
                        ".pathname_offset", // 3
                        ".ref_range",       // 4
                        ".virtual_range",   // 5
                        ".prot",            // 6
                        ".itree",           // 7
                    ];
                    let found_options = find_multiple_option(trimmed, suffix, &options)?;

                    if found_options.contains(&0) {
                        RawCommand::Pheader(RawPheaderCmd::All)
                    } else if found_options.contains(&1) {
                        if range.is_some() || found_options.len() > 1 {
                            return Err(anyhow::anyhow!(
                                "length option is incompatible with the other options"
                            ));
                        }

                        RawCommand::Pheader(RawPheaderCmd::Len)
                    } else {
                        let mut selector = RawPheaderSelector {
                            data: false,
                            pathname_offset: false,
                            ref_range: false,
                            virtual_range: false,
                            prot: false,
                            itree: false,
                        };

                        if found_options.contains(&2) {
                            selector.data = true;
                        }
                        if found_options.contains(&3) {
                            selector.pathname_offset = true;
                        }
                        if found_options.contains(&4) {
                            selector.ref_range = true;
                        }
                        if found_options.contains(&5) {
                            selector.virtual_range = true;
                        }
                        if found_options.contains(&6) {
                            selector.prot = true;
                        }
                        if found_options.contains(&7) {
                            selector.itree = true;
                        }

                        RawCommand::Pheader(RawPheaderCmd::Selector { range, selector })
                    }
                } else {
                    return Err(anyhow::anyhow!("unknown selector {}", trimmed));
                }
            }
            None => RawCommand::Jif { data: false },
        })
    }
}
