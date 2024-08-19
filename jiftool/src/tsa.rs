use jif::itree::interval::DataSource;
use jif::ord::OrdChunk;
use jif::Jif;
use tracer_format::TimestampedAccess;

/// construct the ord chunks from the timestamped log
pub(crate) fn construct_ord_chunks(jif: &Jif, log: Vec<TimestampedAccess>) -> Vec<OrdChunk> {
    let mut chunk = OrdChunk::new(0, 0, DataSource::Zero);
    let mut chunks = Vec::with_capacity(log.len());
    for tsa in log {
        // check if we can merge (empty chunk is always mergeable)
        if !chunk.merge_page(jif, tsa.addr as u64) {
            // we couldn't merge, push the chunk
            chunks.push(chunk);

            let iv = jif.resolve(tsa.addr as u64);
            if iv.is_none() {
                println!("Warning: unresolved address in ordering data: {}", tsa.addr);
                continue;
            }

            chunk = OrdChunk::new(tsa.addr as u64, 1 /* n pages */, iv.unwrap().source);
        }
    }

    if !chunk.is_empty() {
        chunks.push(chunk)
    }

    chunks
}
