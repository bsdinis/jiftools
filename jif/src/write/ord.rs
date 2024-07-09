use crate::error::*;
use crate::ord::OrdChunk;

use std::io::Write;

impl OrdChunk {
    /// Write an ordering chunk
    pub fn to_writer<W: Write>(&self, w: &mut W) -> JifResult<usize> {
        let packed: u64 = (self.vaddr & !0xfff) | ((self.n_pages & 0xfff) as u64);
        w.write_all(&packed.to_le_bytes())?;
        Ok(OrdChunk::serialized_size())
    }
}
