use crate::utils::*;

pub(crate) const MATERIALIZED_COMMAND_USAGE: &str = "materialized command: selection over the materialized JIF representation

jif                                select the whole JIF
jif.strings                        strings in the JIF
jif.zero_pages                     number of zero pages
jif.private_pages                  number of private pages in the JIF
jif.shared_pages                   number of shared pages in the pheader
jif.pages                          total number of pages
jif.intervals                      number of total intervals in the jif (counting implicit intervals)
jif.private_intervals              number of private intervals in the jif (all explicit)
jif.shared_intervals               number of shared intervals in the jif (all implicit)
jif.zero_intervals                 number of zero intervals in the jif (counting implicit intervals)

ord                                select all the ord chunks
ord[<range>]                       select the ord chunks in the range
ord.len                            number of ord chunks
ord.vmas                           number of vmas in the ordering section
ord.pages                          number of pages in the ordering section
ord.private_pages                  number of private pages in the ordering section
ord.shared_pages                   number of shared pages in the ordering section
ord.zero_pages                     number of zero pages in the ordering section
ord.intervals                      number of intervals in the ordering section
ord.private_intervals              number of private intervals in the ordering section
ord.shared_intervals               number of shared intervals in the ordering section
ord.zero_intervals                 number of zero intervals in the ordering section

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
pub(crate) struct RegionTypeSelector {
    pub(crate) zero: bool,
    pub(crate) private: bool,
    pub(crate) shared: bool,
    pub(crate) total: bool,
}

#[derive(Debug)]
pub(crate) enum JifCmd {
    All,
    Strings,
    Pages(RegionTypeSelector),
    Intervals(RegionTypeSelector),
}

#[derive(Debug)]
pub(crate) enum OrdCmd {
    All,
    Range(IndexRange),
    // Number of intervals
    Len,
    Files,
    Vmas,
    // Number of pages
    Pages(RegionTypeSelector),
    // Number of intervals
    Intervals(RegionTypeSelector),
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
jif.metadata                       size of the metadata section (headers + strings + itrees + ord)
jif.data                           size of the data section

strings                            select the strings in the JIF

itrees                             select all the interval trees
itrees[<range>]                    select the interval trees in the range
itrees.len                         number of interval trees

ord                                select all the ord chunks
ord[<range>]                       select the ord chunks in the range
ord.len                            number of ord chunks
ord.size                           number of pages in the ordering section
ord.private_pages                  number of private pages in the ordering section
ord.shared_pages                   number of shared pages in the ordering section
ord.zero_pages                     number of shared pages in the ordering section

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
    Metadata,
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

impl TryFrom<String> for MaterializedCommand {
    type Error = anyhow::Error;
    fn try_from(cmd: String) -> Result<Self, Self::Error> {
        let trimmed = cmd.trim();
        Ok({
            if trimmed.starts_with("jif") {
                let (_prefix, suffix) = trimmed.split_at("jif".len());

                let options = [
                    "",                   // 0
                    ".strings",           // 1
                    ".zero_pages",        // 2
                    ".private_pages",     // 3
                    ".shared_pages",      // 4
                    ".pages",             // 5
                    ".intervals",         // 6
                    ".private_intervals", // 7
                    ".shared_intervals",  // 8
                    ".zero_intervals",    // 9
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
                } else if found_options.contains(&6)
                    || found_options.contains(&7)
                    || found_options.contains(&8)
                    || found_options.contains(&9)
                {
                    let mut selector = RegionTypeSelector::default();
                    if found_options.contains(&6) {
                        selector.total = true;
                    }
                    if found_options.contains(&7) {
                        selector.private = true;
                    }
                    if found_options.contains(&8) {
                        selector.shared = true;
                    }
                    if found_options.contains(&9) {
                        selector.zero = true;
                    }

                    MaterializedCommand::Jif(JifCmd::Intervals(selector))
                } else {
                    let mut selector = RegionTypeSelector::default();
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
                    let options = [
                        "",                   // 0
                        ".len",               // 1
                        ".files",             // 2
                        ".vmas",              // 3
                        ".pages",             // 4
                        ".private_pages",     // 5
                        ".shared_pages",      // 6
                        ".zero_pages",        // 7
                        ".intervals",         // 8
                        ".private_intervals", // 9
                        ".shared_intervals",  // 10
                        ".zero_intervals",    // 11
                    ];
                    let found_options = find_multiple_option(trimmed, suffix, &options)?;

                    if found_options.contains(&0) {
                        MaterializedCommand::Ord(OrdCmd::All)
                    } else if found_options.contains(&1) {
                        if found_options.len() > 1 {
                            return Err(anyhow::anyhow!(
                                "len option is incompatible with the other options"
                            ));
                        }

                        MaterializedCommand::Ord(OrdCmd::Len)
                    } else if found_options.contains(&2) {
                        if found_options.len() > 1 {
                            return Err(anyhow::anyhow!(
                                "files option is incompatible with the other options"
                            ));
                        }

                        MaterializedCommand::Ord(OrdCmd::Files)
                    } else if found_options.contains(&3) {
                        if found_options.len() > 1 {
                            return Err(anyhow::anyhow!(
                                "vmas option is incompatible with the other options"
                            ));
                        }

                        MaterializedCommand::Ord(OrdCmd::Vmas)
                    } else if found_options.contains(&4)
                        || found_options.contains(&5)
                        || found_options.contains(&6)
                        || found_options.contains(&7)
                    {
                        let mut selector = RegionTypeSelector::default();
                        if found_options.contains(&4) {
                            selector.total = true;
                        }
                        if found_options.contains(&5) {
                            selector.private = true;
                        }
                        if found_options.contains(&6) {
                            selector.shared = true;
                        }
                        if found_options.contains(&7) {
                            selector.zero = true;
                        }

                        MaterializedCommand::Ord(OrdCmd::Pages(selector))
                    } else {
                        let mut selector = RegionTypeSelector::default();
                        if found_options.contains(&8) {
                            selector.total = true;
                        }
                        if found_options.contains(&9) {
                            selector.private = true;
                        }
                        if found_options.contains(&10) {
                            selector.shared = true;
                        }
                        if found_options.contains(&11) {
                            selector.zero = true;
                        }

                        MaterializedCommand::Ord(OrdCmd::Intervals(selector))
                    }
                }
            } else {
                if !trimmed.starts_with("pheader") {
                    return Err(anyhow::anyhow!("unknown command {trimmed}"));
                }
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
            }
        })
    }
}

impl TryFrom<String> for RawCommand {
    type Error = anyhow::Error;
    fn try_from(cmd: String) -> Result<Self, Self::Error> {
        Ok({
            let trimmed = cmd.trim();

            if trimmed.starts_with("jif") {
                let (_prefix, suffix) = trimmed.split_at("jif".len());

                let options = ["", ".metadata", ".data"];
                let idx = find_single_option(trimmed, suffix, &options)?;

                if options[idx] == ".data" {
                    RawCommand::Jif(RawJifCmd::Data)
                } else if options[idx] == ".metadata" {
                    RawCommand::Jif(RawJifCmd::Metadata)
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
                    let options = [
                        "",               // 0
                        ".len",           // 1
                        ".pages",         // 2
                        ".private_pages", // 3
                        ".shared_pages",  // 4
                        ".zero_pages",    // 5
                    ];
                    let found_options = find_multiple_option(trimmed, suffix, &options)?;

                    if found_options.contains(&0) {
                        RawCommand::Ord(OrdCmd::All)
                    } else if found_options.contains(&1) {
                        if found_options.len() > 1 {
                            return Err(anyhow::anyhow!(
                                "len option is incompatible with the other options"
                            ));
                        }

                        RawCommand::Ord(OrdCmd::Len)
                    } else {
                        let mut selector = RegionTypeSelector::default();
                        if found_options.contains(&2) {
                            selector.total = true;
                        }
                        if found_options.contains(&3) {
                            selector.private = true;
                        }
                        if found_options.contains(&4) {
                            selector.shared = true;
                        }
                        if found_options.contains(&5) {
                            selector.zero = true;
                        }

                        RawCommand::Ord(OrdCmd::Pages(selector))
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
        })
    }
}
