use jif::ord::OrdChunk;
use jif::Jif;
use tracer_format::TimestampedAccess;

use std::collections::HashMap;

/// sort, truncate addr and do a basic deduplication sweep
pub(crate) fn process_tsa_log(log: Vec<TimestampedAccess>) -> Vec<TimestampedAccess> {
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

/// construct the ord chunks from the timestamped log
pub(crate) fn construct_ord_chunks(jif: &Jif, log: Vec<TimestampedAccess>) -> Vec<OrdChunk> {
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
