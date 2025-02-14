use crate::itree::interval::DataSource;
use crate::ord::OrdChunk;
use crate::ord::{ORD_FLAG_MASK, ORD_FLAG_WRITE, ORD_PRIVATE_FLAG, ORD_SHARED_FLAG, ORD_ZERO_FLAG};
use std::io::Write;

impl OrdChunk {
    /// Write an ordering chunk
    pub fn to_writer<W: Write>(&self, w: &mut W) -> std::io::Result<usize> {
        let mut vaddr = self.vaddr;
        assert!((vaddr & !ORD_FLAG_MASK) == 0);
        vaddr |= match self.kind {
            DataSource::Zero => ORD_ZERO_FLAG,
            DataSource::Private => ORD_PRIVATE_FLAG,
            DataSource::Shared => ORD_SHARED_FLAG,
        };
        if self.is_written_to {
            vaddr |= ORD_FLAG_WRITE
        }
        w.write_all(&vaddr.to_le_bytes())?;
        w.write_all(&self.n_pages.to_le_bytes())?;
        Ok(OrdChunk::serialized_size())
    }
}
