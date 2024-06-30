use crate::error::*;
use crate::ord::OrdChunk;
use crate::utils::read_u64;
use std::io::Read;

impl OrdChunk {
    pub(crate) fn from_reader<R: Read>(r: &mut R) -> JifResult<Self> {
        let mut buffer = [0u8; 8];
        let vaddr_and_n_pages = read_u64(r, &mut buffer)?;
        let vaddr = vaddr_and_n_pages & !0xfff;
        let n_pages = vaddr_and_n_pages as u16 & 0xfff;
        Ok(OrdChunk::new(vaddr, n_pages))
    }
}
