use jif::ord::OrdChunk;
use jif::Jif;
use tracer_format::TimestampedAccess;

fn to_ord_chunk(jif: &Jif, access: TimestampedAccess) -> Option<OrdChunk> {
    jif.resolve(access.masked_addr() as u64).map(|interval| {
        OrdChunk::new(
            access.usecs as u64,
            access.raw_addr() as u64,
            1,
            interval.source,
        )
    })
}

/// construct the ord chunks from the timestamped log
pub(crate) fn construct_ord_chunks(jif: &Jif, log: Vec<TimestampedAccess>) -> Vec<OrdChunk> {
    log.into_iter()
        .filter_map(|access| to_ord_chunk(jif, access))
        .collect()
}
