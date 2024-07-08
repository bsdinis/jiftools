use crate::error::*;
use crate::itree::ITreeNode;
use crate::jif::{JifRaw, JIF_MAGIC_HEADER};
use crate::ord::OrdChunk;
use crate::pheader::JifRawPheader;
use crate::utils::{is_page_aligned, read_u32, seek_to_page};

use std::io::{BufReader, Read, Seek};

impl JifRaw {
    pub fn from_reader<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<Self> {
        let header = JifHeader::from_reader(r)?;

        let pheaders = (0..header.n_pheaders)
            .map(|idx| JifRawPheader::from_reader(r, idx as usize))
            .collect::<Result<Vec<_>, _>>()?;

        let n_itree_nodes = pheaders
            .iter()
            .map(|h| h.itree_n_nodes as usize)
            .sum::<usize>();

        for (pheader_idx, p) in pheaders.iter().enumerate() {
            if p.pathname_offset != u32::MAX && p.pathname_offset >= header.strings_size {
                return Err(JifError::BadPheader {
                    pheader_idx,
                    pheader_err: PheaderError::InvalidOffset {
                        offset: p.pathname_offset,
                        size: header.strings_size,
                    },
                });
            } else if p.itree_n_nodes > 0
                && p.itree_idx.saturating_add(p.itree_n_nodes) as usize > n_itree_nodes
            {
                return Err(JifError::BadPheader {
                    pheader_idx,
                    pheader_err: PheaderError::InvalidITreeIndex {
                        index: p.itree_idx,
                        tree_len: p.itree_n_nodes,
                        len: n_itree_nodes,
                    },
                });
            }
        }

        seek_to_page(r)?;

        // read strings
        let strings_backing = {
            let mut s = Vec::with_capacity(header.strings_size as usize);
            let mut string_reader = r.take(header.strings_size as u64);

            string_reader.read_to_end(&mut s)?;
            Ok::<_, JifError>(s)
        }?;

        // read itree nodes
        let to_skip =
            header.itrees_size as i64 - (n_itree_nodes * ITreeNode::serialized_size()) as i64;
        let itree_nodes = (0..n_itree_nodes)
            .map(|idx| ITreeNode::from_reader(r, idx as usize))
            .collect::<Result<Vec<_>, _>>()?;
        r.seek_relative(to_skip)?;

        let n_ords = header.ord_size as usize / OrdChunk::serialized_size();
        let ord_chunks = (0..n_ords)
            .map(|_| OrdChunk::from_reader(r))
            .filter(|o| o.as_ref().map(|x| !x.is_empty()).unwrap_or(true))
            .collect::<Result<Vec<_>, _>>()?;

        let data_offset = seek_to_page(r)?;

        let data_segments = {
            let mut ds = Vec::new();
            r.read_to_end(&mut ds)?;

            Ok::<_, JifError>(ds)
        }?;

        Ok(JifRaw {
            pheaders,
            strings_backing,
            itree_nodes,
            ord_chunks,
            data_offset,
            data_segments,
        })
    }
}

#[derive(Debug)]
struct JifHeader {
    n_pheaders: u32,
    strings_size: u32,
    itrees_size: u32,
    ord_size: u32,
}

impl JifHeader {
    pub fn from_reader<R: Read>(r: &mut R) -> JifResult<Self> {
        let mut buffer = [0u8; 4];
        r.read_exact(&mut buffer)?;

        if buffer != JIF_MAGIC_HEADER {
            return Err(JifError::BadMagic);
        }

        let n_pheaders = read_u32(r, &mut buffer)?;
        let strings_size = read_u32(r, &mut buffer)?;
        if !is_page_aligned(strings_size as u64) {
            return Err(JifError::BadAlignment);
        }
        let itrees_size = read_u32(r, &mut buffer)?;
        if !is_page_aligned(itrees_size as u64) {
            return Err(JifError::BadAlignment);
        }
        let ord_size = read_u32(r, &mut buffer)?;
        if !is_page_aligned(ord_size as u64) {
            return Err(JifError::BadAlignment);
        }

        Ok(JifHeader {
            n_pheaders,
            strings_size,
            itrees_size,
            ord_size,
        })
    }
}
