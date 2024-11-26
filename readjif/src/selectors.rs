use crate::utils::*;

pub(crate) const MATERIALIZED_COMMAND_USAGE: &str = "materialized command: selection over the materialized JIF representation

jif                                select the whole JIF
jif.strings                        strings in the JIF
jif.zero_pages                     number of zero pages
jif.private_pages                  number of private pages in the JIF
jif.shared_pages                   number of shared pages in the pheader
jif.pages                          total number of pages

ord                                select all the ord chunks
ord[<range>]                       select the ord chunks in the range
ord.len                            number of ord chunks
ord.size                           number of pages in the ordering section

pheader                            select all the pheaders
pheader[<range>]                   select the pheaders in the range
pheader.len                        number of pheaders
pheader.data_size                  size of the data region (mixable with range and other selectors)
pheader.pathname                   reference pathname (mixable with range and other selectors)
pheader.ref_offset                 offset into the file
pheader.virtual_range              virtual address range of the pheader (mixable with range and other selectors)
pheader.virtual_size               size of the virtual address range (mixable with range and other selectors)
pheader.prot                       area `rwx` protections (mixable with range and other selectors)
pheader.itree                      pheader interval tree (mixable with range and other selectors)
pheader.n_itree_nodes              number of interval tree nodes in pheader (mixable with range and other selectors)
pheader.zero_pages                 number of zero pages
pheader.private_pages              == data_size % PAGE_SIZE
pheader.shared_pages               number of shared pages in the pheader
pheader.pages                      total number of pages
";

#[derive(Debug)]
pub(crate) enum MaterializedCommand {
    Ord(OrdCmd),
    Pheader(PheaderCmd),
    Jif(JifCmd),
}

#[derive(Debug, Default)]
pub(crate) struct PageSelector {
    pub(crate) zero: bool,
    pub(crate) private: bool,
    pub(crate) shared: bool,
    pub(crate) total: bool,
}

#[derive(Debug)]
pub(crate) enum JifCmd {
    All,
    Strings,
    Pages(PageSelector),
}

#[derive(Debug)]
pub(crate) enum OrdCmd {
    All,
    Range(IndexRange),
    Len,
    Size,
}

#[derive(Debug, Default)]
pub(crate) struct PheaderSelector {
    pub(crate) virtual_range: bool,
    pub(crate) virtual_size: bool,
    pub(crate) data_size: bool,
    pub(crate) pathname: bool,
    pub(crate) ref_offset: bool,
    pub(crate) prot: bool,
    pub(crate) itree: bool,
    pub(crate) n_itree_nodes: bool,
    pub(crate) zero_pages: bool,
    pub(crate) private_pages: bool,
    pub(crate) shared_pages: bool,
    pub(crate) pages: bool,
}

#[derive(Debug)]
pub(crate) enum PheaderCmd {
    Len,
    Selector {
        range: IndexRange,
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
ord.size                           number of pages in the ordering section

pheader                            select all the pheaders
pheader[<range>]                   select the pheaders in the range
pheader.len                        number of pheaders
pheader.pathname_offset            reference pathname (mixable with range and other selectors)
pheader.ref_offset                 offset into the file
pheader.virtual_range              virtual address range of the pheader (mixable with range and other selectors)
pheader.virtual_size               size of the virtual address range (mixable with range and other selectors)
pheader.prot                       area `rwx` protections (mixable with range and other selectors)
pheader.itree                      show the interval tree offset and size in number of nodes (mixable with range and other selectors)
";

#[derive(Debug)]
pub(crate) enum RawCommand {
    Ord(OrdCmd),
    Pheader(RawPheaderCmd),
    Strings,
    ITree(ITreeCmd),
    Jif(RawJifCmd),
}

#[derive(Debug)]
pub(crate) enum RawJifCmd {
    All,
    Data,
}

#[derive(Debug)]
pub(crate) enum ITreeCmd {
    All,
    Range(IndexRange),
    Len,
}

#[derive(Debug, Default)]
pub(crate) struct RawPheaderSelector {
    pub(crate) virtual_range: bool,
    pub(crate) virtual_size: bool,
    pub(crate) pathname_offset: bool,
    pub(crate) ref_offset: bool,
    pub(crate) prot: bool,
    pub(crate) itree: bool,
}

#[derive(Debug)]
pub(crate) enum RawPheaderCmd {
    Len,
    Selector {
        range: IndexRange,
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

                    let options = [
                        "",               // 0
                        ".strings",       // 1
                        ".zero_pages",    // 2
                        ".private_pages", // 3
                        ".shared_pages",  // 4
                        ".pages",         // 5
                    ];
                    let found_options = find_multiple_option(trimmed, suffix, &options)?;

                    if found_options.contains(&0) {
                        MaterializedCommand::Jif(JifCmd::All)
                    } else if found_options.contains(&1) {
                        if found_options.len() > 1 {
                            return Err(anyhow::anyhow!(
                                "strings option is incompatible with the other options"
                            ));
                        }

                        MaterializedCommand::Jif(JifCmd::Strings)
                    } else {
                        let mut selector = PageSelector::default();
                        if found_options.contains(&2) {
                            selector.zero = true;
                        }
                        if found_options.contains(&3) {
                            selector.private = true;
                        }
                        if found_options.contains(&4) {
                            selector.shared = true;
                        }
                        if found_options.contains(&5) {
                            selector.total = true;
                        }

                        MaterializedCommand::Jif(JifCmd::Pages(selector))
                    }
                } else if trimmed.starts_with("ord") {
                    let (_prefix, suffix) = trimmed.split_at("ord".len());
                    let (range, suffix) = find_range(trimmed, suffix)?;

                    if range.is_some() {
                        if !suffix.is_empty() {
                            return Err(anyhow::anyhow!(
                                "trailing data after range in {}: {}",
                                trimmed,
                                suffix
                            ));
                        }

                        MaterializedCommand::Ord(OrdCmd::Range(range))
                    } else {
                        let options = ["", ".len", ".size"];
                        let idx = find_single_option(trimmed, suffix, &options)?;
                        if options[idx] == ".len" {
                            MaterializedCommand::Ord(OrdCmd::Len)
                        } else if options[idx] == ".size" {
                            MaterializedCommand::Ord(OrdCmd::Size)
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
                        ".virtual_range", // 2
                        ".virtual_size",  // 3
                        ".data_size",     // 4
                        ".pathname",      // 5
                        ".ref_offset",    // 6
                        ".prot",          // 7
                        ".itree",         // 8
                        ".n_itree_nodes", // 9
                        ".zero_pages",    // 10
                        ".private_pages", // 11
                        ".shared_pages",  // 12
                        ".pages",         // 13
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
                        let mut selector = PheaderSelector::default();

                        if found_options.contains(&2) {
                            selector.virtual_range = true;
                        }
                        if found_options.contains(&3) {
                            selector.virtual_size = true;
                        }
                        if found_options.contains(&4) {
                            selector.data_size = true;
                        }
                        if found_options.contains(&5) {
                            selector.pathname = true;
                        }
                        if found_options.contains(&6) {
                            selector.ref_offset = true;
                        }
                        if found_options.contains(&7) {
                            selector.prot = true;
                        }
                        if found_options.contains(&8) {
                            selector.itree = true;
                        }
                        if found_options.contains(&9) {
                            selector.n_itree_nodes = true;
                        }
                        if found_options.contains(&10) {
                            selector.zero_pages = true;
                        }
                        if found_options.contains(&11) {
                            selector.private_pages = true;
                        }
                        if found_options.contains(&12) {
                            selector.shared_pages = true;
                        }
                        if found_options.contains(&13) {
                            selector.pages = true;
                        }

                        MaterializedCommand::Pheader(PheaderCmd::Selector { range, selector })
                    }
                } else {
                    return Err(anyhow::anyhow!("unknown selector {}", trimmed));
                }
            }
            None => MaterializedCommand::Jif(JifCmd::All),
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

                    if options[idx] == ".data" {
                        RawCommand::Jif(RawJifCmd::Data)
                    } else {
                        RawCommand::Jif(RawJifCmd::All)
                    }
                } else if trimmed.starts_with("strings") {
                    let (_prefix, suffix) = trimmed.split_at("strings".len());

                    let options = [""];
                    let _idx = find_single_option(trimmed, suffix, &options)?;
                    RawCommand::Strings
                } else if trimmed.starts_with("ord") {
                    let (_prefix, suffix) = trimmed.split_at("ord".len());
                    let (range, suffix) = find_range(trimmed, suffix)?;

                    if range.is_some() {
                        if !suffix.is_empty() {
                            return Err(anyhow::anyhow!(
                                "trailing data after range in {}: {}",
                                trimmed,
                                suffix
                            ));
                        }

                        RawCommand::Ord(OrdCmd::Range(range))
                    } else {
                        let options = ["", ".len", ".size"];
                        let idx = find_single_option(trimmed, suffix, &options)?;
                        if options[idx] == ".len" {
                            RawCommand::Ord(OrdCmd::Len)
                        } else if options[idx] == ".size" {
                            RawCommand::Ord(OrdCmd::Size)
                        } else {
                            RawCommand::Ord(OrdCmd::All)
                        }
                    }
                } else if trimmed.starts_with("itree") {
                    let (_prefix, suffix) = trimmed.split_at("itree".len());
                    let (range, suffix) = find_range(trimmed, suffix)?;

                    if range.is_some() {
                        if !suffix.is_empty() {
                            return Err(anyhow::anyhow!(
                                "trailing data after range in {}: {}",
                                trimmed,
                                suffix
                            ));
                        }

                        RawCommand::ITree(ITreeCmd::Range(range))
                    } else {
                        let options = ["", ".len"];
                        let idx = find_single_option(trimmed, suffix, &options)?;
                        if options[idx] == ".len" {
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
                        ".virtual_range",   // 2
                        ".virtual_size",    // 3
                        ".pathname_offset", // 4
                        ".ref_offset",      // 5
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
                        let mut selector = RawPheaderSelector::default();

                        if found_options.contains(&2) {
                            selector.virtual_range = true;
                        }
                        if found_options.contains(&3) {
                            selector.virtual_size = true;
                        }
                        if found_options.contains(&4) {
                            selector.pathname_offset = true;
                        }
                        if found_options.contains(&5) {
                            selector.ref_offset = true;
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
            None => RawCommand::Jif(RawJifCmd::All),
        })
    }
}
