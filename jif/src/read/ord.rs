use crate::error::*;
use crate::itree::interval::DataSource;
use crate::ord::{OrdChunk, ORD_FLAG_WRITE};
use crate::ord::{ORD_FLAG_MASK, ORD_PRIVATE_FLAG, ORD_SHARED_FLAG, ORD_ZERO_FLAG};
use crate::utils::{is_page_aligned, read_u64};
use std::io::Read;

impl OrdChunk {
    /// Read and parse an OrdChunk
    pub fn from_reader<R: Read>(r: &mut R) -> OrdChunkResult<Self> {
        let mut buffer = [0u8; 8];
        let vaddr = read_u64(r, &mut buffer)?;
        if !is_page_aligned(vaddr) {
            return Err(OrdChunkError::BadAlignment(vaddr));
        }

        let write_first = (vaddr & ORD_FLAG_WRITE) != 0;

        let kind = match vaddr & !ORD_FLAG_MASK {
            ORD_ZERO_FLAG => DataSource::Zero,
            ORD_PRIVATE_FLAG => DataSource::Private,
            ORD_SHARED_FLAG => DataSource::Shared,
            0 => {
                assert!(vaddr == 0);
                DataSource::Zero
            }
            _ => panic!("bad flag"),
        };

        let n_pages = read_u64(r, &mut buffer)?;
        Ok(OrdChunk {
            vaddr: vaddr & ORD_FLAG_MASK,
            n_pages,
            kind,
            is_written_to: write_first,
        })
    }
}
