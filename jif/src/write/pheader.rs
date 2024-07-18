use crate::pheader::JifRawPheader;

use std::io::Write;

impl JifRawPheader {
    /// Write a pheader
    pub fn to_writer<W: Write>(&self, w: &mut W) -> std::io::Result<usize> {
        w.write_all(&self.vbegin.to_le_bytes())?;
        w.write_all(&self.vend.to_le_bytes())?;
        w.write_all(&self.ref_offset.to_le_bytes())?;
        w.write_all(&self.itree_idx.to_le_bytes())?;
        w.write_all(&self.itree_n_nodes.to_le_bytes())?;
        w.write_all(&self.pathname_offset.to_le_bytes())?;
        w.write_all(&self.prot.to_le_bytes())?;

        Ok(JifRawPheader::serialized_size())
    }
}
