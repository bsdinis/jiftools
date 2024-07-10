use crate::error::*;
use crate::itree_node::RawITreeNode;
use crate::jif::{JifRaw, JIF_MAGIC_HEADER};
use crate::ord::OrdChunk;
use crate::utils::{is_page_aligned, page_align, PAGE_SIZE};

use std::io::Write;

impl JifRaw {
    /// Write a JIF
    pub fn to_writer<W: Write>(&self, w: &mut W) -> JifResult<usize> {
        fn write_to_page_alignment<W: Write>(
            w: &mut W,
            cursor: usize,
            buffer: &[u8; PAGE_SIZE],
        ) -> JifResult<usize> {
            let delta = page_align(cursor as u64) as usize - cursor;
            if delta > 0 {
                w.write_all(&buffer[..delta])?;
            }

            Ok(delta)
        }

        let zero_page = [0u8; PAGE_SIZE];
        let ones_page = [0xffu8; PAGE_SIZE];

        let n_pheaders = self.pheaders.len() as u32;
        let strings_size = page_align(self.strings_backing.len() as u64) as u32;
        let itrees_size =
            page_align((self.itree_nodes.len() * RawITreeNode::serialized_size()) as u64) as u32;
        let ord_size =
            page_align((self.ord_chunks.len() * OrdChunk::serialized_size()) as u64) as u32;

        let mut cursor = 0;

        // dump header
        w.write_all(&JIF_MAGIC_HEADER)?;
        w.write_all(&n_pheaders.to_le_bytes())?;
        w.write_all(&strings_size.to_le_bytes())?;
        w.write_all(&itrees_size.to_le_bytes())?;
        w.write_all(&ord_size.to_le_bytes())?;

        cursor += JIF_MAGIC_HEADER.len()
            + std::mem::size_of::<u32>() // n_pheaders
            + std::mem::size_of::<u32>() // strings_size
            + std::mem::size_of::<u32>() // itrees_size
            + std::mem::size_of::<u32>(); // ord_size

        // pheaders
        for pheader in &self.pheaders {
            cursor += pheader.to_writer(w)?;
        }

        let written = write_to_page_alignment(w, cursor, &zero_page)?;
        cursor += written;

        // strings
        w.write_all(&self.strings_backing)?;
        cursor += self.strings_backing.len();

        let written = write_to_page_alignment(w, cursor, &zero_page)?;
        cursor += written;

        // itree nodes
        for node in &self.itree_nodes {
            cursor += node.to_writer(w)?;
        }

        let written = write_to_page_alignment(w, cursor, &ones_page)?;
        cursor += written;

        // ord chunks
        for ord in &self.ord_chunks {
            cursor += ord.to_writer(w)?;
        }
        let written = write_to_page_alignment(w, cursor, &zero_page)?;
        cursor += written;

        if cursor != self.data_offset as usize {
            eprintln!(
                "WARN: cursor ({:#x}) did not match up with expected data offset ({:#x})",
                cursor, self.data_offset
            );

            assert!(cursor < self.data_offset as usize);

            if !is_page_aligned(cursor as u64) {
                eprintln!("WARN: cursor ({:#x}) should be page aligned by now", cursor);
                let written = write_to_page_alignment(w, cursor, &zero_page)?;
                cursor += written;
            }

            let n_pages = (self.data_offset as usize - cursor) / PAGE_SIZE;

            for _ in 0..n_pages {
                w.write_all(&zero_page)?;
                cursor += zero_page.len();
            }
        }

        w.write_all(&self.data_segments)?;
        cursor += self.data_segments.len();
        Ok(cursor)
    }
}
