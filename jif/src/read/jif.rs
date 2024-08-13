use crate::error::*;
use crate::itree::itree_node::RawITreeNode;
use crate::jif::{JifRaw, JIF_MAGIC_HEADER, JIF_VERSION};
use crate::ord::OrdChunk;
use crate::pheader::JifRawPheader;
use crate::utils::{is_page_aligned, read_u32, read_u64, seek_to_page};

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Read, Seek};

impl JifRaw {
    /// Read and parse a JIF
    pub fn from_reader<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<Self> {
        let header = JifHeader::from_reader(r)?;

        let pheaders = (0..(header.n_pheaders as usize))
            .map(|pheader_idx| {
                JifRawPheader::from_reader(r).map_err(|pheader_err| JifError::BadPheader {
                    pheader_idx,
                    pheader_err,
                })
            })
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
            header.itrees_size as i64 - (n_itree_nodes * RawITreeNode::serialized_size()) as i64;
        let itree_nodes = (0..n_itree_nodes)
            .map(|itree_node_idx| {
                RawITreeNode::from_reader(r).map_err(|itree_node_err| JifError::BadITreeNode {
                    itree_node_idx,
                    itree_node_err,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        r.seek_relative(to_skip)?;

        // read ord segments
        let n_ords = header.ord_size as usize / OrdChunk::serialized_size();
        let ord_chunks = (0..n_ords)
            .map(|ord_chunk_idx| {
                OrdChunk::from_reader(r).map_err(|ord_chunk_err| JifError::BadOrdChunk {
                    ord_chunk_idx,
                    ord_chunk_err,
                })
            })
            .filter(|o| o.as_ref().map(|x| !x.is_empty()).unwrap_or(true))
            .collect::<Result<Vec<_>, _>>()?;

        let data_offset = seek_to_page(r)?;

        // read data segments
        let data_segments = {
            // deduplicated intervals can issue the same data ranges
            // we need to deduplicate them here
            let data_offset_intervals = itree_nodes
                .iter()
                .flat_map(|n| n.ranges.iter())
                .filter(|i| i.is_data())
                .map(|i| (i.offset - data_offset, i.len()))
                .collect::<BTreeSet<_>>();

            for (ival1, ival2) in data_offset_intervals
                .iter()
                .zip(data_offset_intervals.iter().skip(1))
            {
                assert_eq!(
                    ival1.0 + ival1.1,
                    ival2.0,
                    "intervals are not contiguous: [{:#x}; {:#x}) and [{:#x}; {:#x})",
                    ival1.0,
                    ival1.0 + ival1.1,
                    ival2.0,
                    ival2.0 + ival2.1,
                );
            }

            let mut map = BTreeMap::new();
            for (offset, len) in data_offset_intervals {
                let data = {
                    let mut d = Vec::new();
                    let mut reader = r.take(len);
                    reader.read_to_end(&mut d)?;
                    Ok::<Vec<_>, std::io::Error>(d)
                }?;

                map.insert((offset, offset + len), data);
            }

            Ok::<_, JifError>(map)
        }?;

        Ok(JifRaw {
            pheaders,
            strings_backing,
            itree_nodes,
            ord_chunks,
            data_offset,
            data_segments,
            n_prefetch: header.n_prefetch,
        })
    }
}

#[derive(Debug)]
struct JifHeader {
    n_pheaders: u32,
    strings_size: u32,
    itrees_size: u32,
    ord_size: u32,
    n_prefetch: u64,
}

impl JifHeader {
    /// Read and parse a JIF header
    fn from_reader<R: Read>(r: &mut R) -> JifResult<Self> {
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

        let version = read_u32(r, &mut buffer)?;
        if version != JIF_VERSION {
            return Err(JifError::BadVersion);
        }

        let mut buffer = [0u8; 8];
        let n_prefetch = read_u64(r, &mut buffer)?;

        Ok(JifHeader {
            n_pheaders,
            strings_size,
            itrees_size,
            ord_size,
            n_prefetch,
        })
    }
}
