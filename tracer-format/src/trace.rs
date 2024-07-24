use crate::error::TraceReadError;
use crate::timestamped_access::TimestampedAccess;

use std::collections::HashMap;
use std::io::BufRead;

/// Read a full recorded trace
pub fn read_trace<BR: BufRead>(reader: BR) -> Result<Vec<TimestampedAccess>, TraceReadError> {
    reader
        .lines()
        .enumerate()
        .map(|(idx, line)| {
            (line?)
                .parse()
                .map_err(|error| TraceReadError::ParseError { line: idx, error })
        })
        .collect::<Result<Vec<_>, _>>()
}

/// Dedup and sort a trace
pub fn dedup_and_sort(log: Vec<TimestampedAccess>) -> Vec<TimestampedAccess> {
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

    let mut log = map.into_values().collect::<Vec<_>>();
    log.sort_by_key(|tsa| tsa.usecs);

    log
}

// TODO: add tests
