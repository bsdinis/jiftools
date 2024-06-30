use crate::error::*;
use crate::itree::{ITree, ITreeNode};
use crate::ord::OrdChunk;
use crate::pheader::{JifPheader, JifRawPheader};
use crate::utils::{is_page_aligned, read_u32, seek_to_page};

use std::io::{BufReader, Read, Seek};
use std::str::from_utf8;

pub(crate) const JIF_MAGIC_HEADER: [u8; 4] = [0x77, b'J', b'I', b'F'];

pub struct Jif {
    pub pheaders: Vec<JifPheader>,
    pub ord_chunks: Vec<OrdChunk>,
    pub data_offset: u64,
    pub data_segments: Vec<u8>,
}

/// JIF file representation
///
pub struct JifRaw {
    pheaders: Vec<JifRawPheader>,
    strings_backing: Vec<u8>,
    itree_nodes: Vec<ITreeNode>,
    ord_chunks: Vec<OrdChunk>,
    data_offset: u64,
    data_segments: Vec<u8>,
}

struct JifHeader {
    n_pheaders: u32,
    strings_size: u32,
    itrees_size: u32,
    ord_size: u32,
}

impl Jif {
    pub fn from_raw(raw: JifRaw) -> Self {
        let pheaders = raw
            .pheaders
            .iter()
            .map(|raw_pheader| JifPheader::from_raw(&raw, &raw_pheader))
            .collect::<Vec<JifPheader>>();

        Jif {
            pheaders,
            ord_chunks: raw.ord_chunks,
            data_offset: raw.data_offset,
            data_segments: raw.data_segments,
        }
    }

    pub fn from_reader<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<Self> {
        Ok(Jif::from_raw(JifRaw::from_reader(r)?))
    }
}

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

    pub fn strings(&self) -> Vec<&str> {
        let first_last_zero = self
            .strings_backing
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| **c != 0u8)
            .map(|(idx, _)| std::cmp::min(idx + 1, self.strings_backing.len()))
            .unwrap_or(self.strings_backing.len());

        self.strings_backing[..first_last_zero]
            .split(|x| *x == 0)
            .map(|s| from_utf8(s).unwrap_or("<failed to parse>"))
            .collect::<Vec<&str>>()
    }

    pub fn string_at_offset(&self, offset: usize) -> Option<&str> {
        if offset > self.strings_backing.len() {
            return None;
        }

        self.strings_backing[offset..]
            .split(|x| *x == 0)
            .map(|s| from_utf8(s).unwrap_or("<failed to parse>"))
            .next()
    }

    pub fn get_itree(&self, index: usize, n: usize) -> Option<ITree> {
        if index.saturating_add(n) > self.itree_nodes.len() {
            return None;
        }

        let nodes = self
            .itree_nodes
            .iter()
            .skip(index)
            .take(n)
            .cloned()
            .collect::<Vec<_>>();

        Some(ITree::new(nodes))
    }
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

impl std::fmt::Debug for Jif {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jif")
            .field("pheaders", &self.pheaders)
            .field("ord", &self.ord_chunks)
            .field("data_offset", &self.data_offset)
            .finish()
    }
}

impl std::fmt::Debug for JifRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let strings = self.strings();
        f.debug_struct("Jif")
            .field("pheaders", &self.pheaders)
            .field("strings", &strings)
            .field("itrees", &self.itree_nodes)
            .field("ord", &self.ord_chunks)
            .field("data_offset", &self.data_offset)
            .finish()
    }
}
