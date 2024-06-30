use crate::error::*;
use crate::utils::read_u64;

use std::io::Read;

#[derive(Debug)]
pub struct OrdChunk {
    /// first 42 bits encode the page number of the first page
    pub vaddr: u64,

    /// last 12 bits encode the number of pages
    pub n_pages: u16,
}

impl OrdChunk {
    pub(crate) const fn serialized_size() -> usize {
        std::mem::size_of::<u64>()
    }
    pub(crate) fn from_reader<R: Read>(r: &mut R) -> JifResult<Self> {
        let mut buffer = [0u8; 8];
        let vaddr_and_n_pages = read_u64(r, &mut buffer)?;
        let vaddr = vaddr_and_n_pages & !0xfff;
        let n_pages = vaddr_and_n_pages as u16 & 0xfff;
        Ok(OrdChunk { vaddr, n_pages })
    }
}
