use crate::ord::OrdChunk;

use std::io::Write;

impl OrdChunk {
    /// Write an ordering chunk
    pub fn to_writer<W: Write>(&self, w: &mut W) -> std::io::Result<usize> {
        w.write_all(&self.vaddr.to_le_bytes())?;
        w.write_all(&self.n_pages.to_le_bytes())?;
        Ok(OrdChunk::serialized_size())
    }
}
