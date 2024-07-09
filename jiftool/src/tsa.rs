use jif::*;

use anyhow::Context;
use std::collections::HashMap;
use std::io::BufRead;
use std::str::FromStr;

#[derive(Copy, Clone, Ord)]
pub(crate) struct TimesampedAccess {
    pub(crate) usecs: usize,
    pub(crate) addr: usize,
}

impl TimesampedAccess {
    // make addr page aligned (we only care about pages when prefetching)
    fn truncate_addr(&mut self) {
        self.addr &= !0xfff;
    }
}

impl PartialEq for TimesampedAccess {
    fn eq(&self, other: &Self) -> bool {
        self.usecs == other.usecs && self.addr == other.addr
    }
}
impl Eq for TimesampedAccess {}

impl PartialOrd for TimesampedAccess {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.usecs < other.usecs {
            Some(std::cmp::Ordering::Less)
        } else if self.usecs > other.usecs {
            Some(std::cmp::Ordering::Greater)
        } else {
            // all is equal except the addr
            if self.addr == other.addr {
                Some(std::cmp::Ordering::Equal)
            } else {
                // different addrs are not comparable
                None
            }
        }
    }
}

impl FromStr for TimesampedAccess {
    type Err = anyhow::Error;
    /// parse a line in the log of accesses
    ///
    /// `<usecs>: <address>`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (usec_str, addr_str) = s.split_once(":").ok_or_else(|| {
            anyhow::anyhow!("could not find usecond/address delimiter `:` in `{}`", s)
        })?;

        let usecs = usec_str
            .trim()
            .parse::<usize>()
            .context(format!("failed to parse seconds string: {}", usec_str))?;
        let addr = if let Some(hex_str) = addr_str.trim().strip_prefix("0x") {
            usize::from_str_radix(hex_str, 0x10)
        } else {
            usize::from_str_radix(addr_str.trim(), 10)
        }
        .context(format!("failed to parse address string: {}", addr_str))?;

        Ok(TimesampedAccess { usecs, addr })
    }
}

/// read_ords gets the ordering log of accesses
/// the expected (line) format is
///
/// no other deduplication or coallescing happens at this stage
pub(crate) fn read_tsa_log<BR: BufRead>(reader: BR) -> anyhow::Result<Vec<TimesampedAccess>> {
    reader
        .lines()
        .map(|line| (line?).parse())
        .collect::<Result<Vec<_>, _>>()
}

/// sort, truncate addr and do a basic deduplication sweep
pub(crate) fn process_tsa_log(log: Vec<TimesampedAccess>) -> Vec<TimesampedAccess> {
    // deduping:
    // construct an addr -> access hashmap map, where we keep only the first access
    let mut map = HashMap::with_capacity(log.len());
    for tsa in log.into_iter().map(|mut tsa| {
        tsa.truncate_addr();
        tsa
    }) {
        // keep the most recent entry
        map.entry(tsa.addr)
            .and_modify(|existing| {
                if tsa < *existing {
                    *existing = tsa
                }
            })
            .or_insert(tsa);
    }

    let mut log = map.into_iter().map(|(_addr, tsa)| tsa).collect::<Vec<_>>();
    log.sort();

    log
}

/// construct the ord chunks from the timestamped log
pub(crate) fn construct_ord_chunks(jif: &Jif, log: Vec<TimesampedAccess>) -> Vec<OrdChunk> {
    let mut chunk = OrdChunk::default();
    let mut chunks = Vec::with_capacity(log.len());
    for tsa in log {
        // check if we can merge (empty chunk is always mergeable)
        if !chunk.merge_page(jif, tsa.addr as u64) {
            // we couldn't merge, push the chunk
            chunks.push(chunk);

            chunk = OrdChunk::new(tsa.addr as u64, 1 /* n pages */);
        }
    }

    if !chunk.is_empty() {
        chunks.push(chunk)
    }

    chunks
}
