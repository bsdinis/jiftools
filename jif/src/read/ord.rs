use crate::error::*;
use crate::ord::OrdChunk;
use crate::utils::{is_page_aligned, read_u64};
use std::io::Read;

impl OrdChunk {
    /// Read and parse an OrdChunk
    pub fn from_reader<R: Read>(r: &mut R) -> OrdChunkResult<Self> {
        let mut buffer = [0u8; 8];
        let vaddr = read_u64(r, &mut buffer)?;
        if !is_page_aligned(vaddr) {
            Err(OrdChunkError::BadAlignment(vaddr))
        } else {
            let n_pages = read_u64(r, &mut buffer)?;
            Ok(OrdChunk { vaddr, n_pages })
        }
    }
}
