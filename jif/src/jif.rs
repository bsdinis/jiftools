use crate::error::*;
use crate::itree::ITree;
use crate::itree_node::{ITreeNode, RawITreeNode};
use crate::ord::OrdChunk;
use crate::pheader::{JifPheader, JifRawPheader};
use crate::utils::page_align;

use std::collections::{BTreeMap, HashSet};
use std::io::{BufReader, Read, Seek, Write};
use std::str::from_utf8;

pub(crate) const JIF_MAGIC_HEADER: [u8; 4] = [0x77, b'J', b'I', b'F'];

/// The materialized view over the JIF file
///
/// After materialization the JIF format simplifies greatly:
/// it is simply a list of virtual memory areas (the pheaders)
/// and the ordering list for the prefetcher
pub struct Jif {
    pub(crate) pheaders: Vec<JifPheader>,
    pub(crate) ord_chunks: Vec<OrdChunk>,
}

/// The "raw" JIF file representation
/// This consists of a 1:1 mapping into how the data is layed out on disk
///
pub struct JifRaw {
    pub(crate) pheaders: Vec<JifRawPheader>,
    pub(crate) strings_backing: Vec<u8>,
    pub(crate) itree_nodes: Vec<RawITreeNode>,
    pub(crate) ord_chunks: Vec<OrdChunk>,
    pub(crate) data_offset: u64,
    pub(crate) data_segments: Vec<u8>,
}

impl Jif {
    /// Materialize a `Jif` from its raw counterpart
    pub fn from_raw(mut raw: JifRaw) -> JifResult<Self> {
        fn construct_data_map(raw: &mut JifRaw) -> JifResult<BTreeMap<u64, Vec<u8>>> {
            let intervals = {
                let mut is = raw
                    .itree_nodes
                    .iter()
                    .flat_map(|node| node.ranges().iter().filter(|i| i.is_data()))
                    .cloned()
                    .collect::<Vec<_>>();

                // order by descending order of offsets
                is.sort_by(|i1, i2| i2.offset.cmp(&i1.offset));

                is
            };

            let mut map = BTreeMap::new();
            let mut data_segments = raw.take_data();
            for interval in intervals {
                let data = data_segments.split_off((interval.offset - raw.data_offset) as usize);
                if data.len() < interval.len() as usize {
                    return Err(JifError::DataSegmentNotFound {
                        data_range: (interval.offset, interval.offset + interval.len()),
                        virtual_range: (interval.start, interval.end),
                    });
                }
                map.insert(interval.start, data);
            }

            Ok(map)
        }

        let mut data_map = construct_data_map(&mut raw)?;
        let pheaders = raw
            .pheaders
            .iter()
            .map(|raw_pheader| JifPheader::from_raw(&raw, raw_pheader, &mut data_map))
            .collect::<Result<Vec<JifPheader>, _>>()?;

        Ok(Jif {
            pheaders,
            ord_chunks: raw.ord_chunks,
        })
    }

    /// List out all the strings in the pheaders
    pub fn strings(&self) -> HashSet<&str> {
        self.pheaders
            .iter()
            .filter_map(|phdr| phdr.ref_segment.as_ref().map(|(s, _)| s.as_str()))
            .collect()
    }

    /// Read the `Jif` from a file
    pub fn from_reader<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<Self> {
        Jif::from_raw(JifRaw::from_reader(r)?)
    }

    /// Write the `Jif` to a file
    pub fn to_writer<W: Write>(self, w: &mut W) -> JifResult<usize> {
        let raw = JifRaw::from_materialized(self);
        raw.to_writer(w)
    }

    /// Compute the data offset (i.e., the offset where data starts being laid out)
    pub fn data_offset(&self) -> u64 {
        let header_size = JIF_MAGIC_HEADER.len()
            + std::mem::size_of::<u32>() // n_pheaders
            + std::mem::size_of::<u32>() // strings_size
            + std::mem::size_of::<u32>() // itrees_size
            + std::mem::size_of::<u32>(); // ord_size

        let pheader_size = self.pheaders.len() * JifRawPheader::serialized_size();

        let strings_size = self
            .strings()
            .into_iter()
            .map(|x| x.len() + 1 /* NUL */)
            .sum::<usize>();

        let itree_size = self
            .pheaders
            .iter()
            .map(|phdr| phdr.itree.n_nodes())
            .sum::<usize>()
            * RawITreeNode::serialized_size();

        let ord_size = self.ord_chunks.len() * OrdChunk::serialized_size();

        page_align((header_size + pheader_size) as u64)
            + page_align(strings_size as u64)
            + page_align(itree_size as u64)
            + page_align(ord_size as u64)
    }

    /// Construct the interval trees of all the pheaders
    pub fn build_itrees(&mut self) -> JifResult<()> {
        for p in self.pheaders.iter_mut() {
            p.build_itree()?;
        }

        Ok(())
    }

    /// Rename a file globally
    pub fn rename_file(&mut self, old: &str, new: &str) {
        for p in self.pheaders.iter_mut() {
            p.rename_file(old, new);
        }
    }

    /// Add a new ordering section
    pub fn add_ordering_info(&mut self, ordering_info: Vec<OrdChunk>) -> JifResult<()> {
        fn check_and_process(jif: &Jif, ordering_info: Vec<OrdChunk>) -> JifResult<Vec<OrdChunk>> {
            ordering_info
                .into_iter()
                .filter_map(|chunk| {
                    if chunk.is_empty() {
                        None
                    } else if jif.mapping_pheader_idx(chunk.vaddr).is_none() {
                        Some(Err(JifError::UnmappedOrderingAddr(chunk.vaddr)))
                    } else {
                        Some(Ok(chunk))
                    }
                })
                .collect::<Result<Vec<_>, _>>()
        }

        self.ord_chunks = check_and_process(self, ordering_info)?;
        Ok(())
    }

    /// Access the pheaders
    pub fn pheaders(&self) -> &[JifPheader] {
        &self.pheaders
    }

    /// Stored data size in B
    pub fn date_size(&self) -> usize {
        self.pheaders.iter().map(|phdr| phdr.data_size()).sum()
    }

    /// Access the ordering list
    pub fn ord_chunks(&self) -> &[OrdChunk] {
        &self.ord_chunks
    }

    /// Compute the total number of zero pages encoded (by omission) in the JIF
    pub fn zero_pages(&self) -> usize {
        self.pheaders.iter().map(|phdr| phdr.zero_pages()).sum()
    }

    /// Compute the total number of private pages stored (directly) in the JIF
    pub fn private_pages(&self) -> usize {
        self.pheaders.iter().map(|phdr| phdr.private_pages()).sum()
    }

    /// Compute the total number of shared pages referenced by the JIF
    pub fn shared_pages(&self) -> usize {
        self.pheaders.iter().map(|phdr| phdr.shared_pages()).sum()
    }

    /// The total number of pages
    pub fn total_pages(&self) -> usize {
        self.pheaders.iter().map(|phdr| phdr.total_pages()).sum()
    }

    // Find the pheader (by index) that maps a particular address
    pub(crate) fn mapping_pheader_idx(&self, vaddr: u64) -> Option<usize> {
        self.pheaders
            .iter()
            .enumerate()
            .find(|(_idx, pheader)| pheader.mapps_addr(vaddr))
            .map(|(idx, _pheader)| idx)
    }
}

impl JifRaw {
    /// Construct a raw JIF from a materialized one
    pub fn from_materialized(jif: Jif) -> Self {
        let string_map = {
            let strings = jif
                .pheaders
                .iter()
                .filter_map(|phdr| phdr.ref_segment.as_ref().map(|(s, _)| s.clone()))
                .collect::<HashSet<_>>();

            let mut offset = 0;
            strings
                .into_iter()
                .map(|s| {
                    let r = (s, offset);
                    offset += r.0.len() + 1 /* NUL */;
                    r
                })
                .collect::<BTreeMap<_, _>>()
        };

        let data_offset = jif.data_offset();
        let mut itree_nodes = Vec::new();
        let mut data_segments = Vec::new();
        let pheaders = jif
            .pheaders
            .into_iter()
            .map(|phdr| {
                JifRawPheader::from_materialized(
                    phdr,
                    &string_map,
                    &mut itree_nodes,
                    data_offset,
                    &mut data_segments,
                )
            })
            .collect::<Vec<_>>();

        let strings = {
            let mut m = string_map.into_iter().collect::<Vec<_>>();
            m.sort_by_key(|(_s, off)| *off);
            m
        };

        let strings_size = strings
            .last()
            .map(|(s, off)| off + s.len() + 1 /* NUL */)
            .unwrap_or(0);

        let strings_backing = {
            let mut s = Vec::with_capacity(strings_size);
            for (string, _offset) in strings {
                s.append(&mut string.into_bytes());
                s.push(0); // NUL byte
            }

            s
        };

        JifRaw {
            pheaders,
            strings_backing,
            itree_nodes,
            ord_chunks: jif.ord_chunks,
            data_offset,
            data_segments,
        }
    }

    /// Remove the data from the raw JIF file
    pub fn take_data(&mut self) -> Vec<u8> {
        if self.data_segments.is_empty() {
            return Vec::new();
        }
        self.data_segments.split_off(0)
    }

    /// Access the pheaders
    pub fn pheaders(&self) -> &[JifRawPheader] {
        &self.pheaders
    }

    /// Access the ordering list
    pub fn ord_chunks(&self) -> &[OrdChunk] {
        &self.ord_chunks
    }

    /// Access the interval tree node list
    pub fn itree_nodes(&self) -> &[RawITreeNode] {
        &self.itree_nodes
    }

    /// Access the data segments
    pub fn data(&self) -> &[u8] {
        &self.data_segments
    }

    /// Access the string table
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

    /// Find a string at a particular offset
    pub(crate) fn string_at_offset(&self, offset: usize) -> Option<&str> {
        if offset > self.strings_backing.len() {
            return None;
        }

        self.strings_backing[offset..]
            .split(|x| *x == 0)
            .map(|s| from_utf8(s).unwrap_or("<failed to parse>"))
            .next()
    }

    /// Get an interval tree from an (index, len) range
    pub(crate) fn get_itree(
        &self,
        index: usize,
        n: usize,
        data_map: &mut BTreeMap<u64, Vec<u8>>,
    ) -> JifResult<ITree> {
        if index.saturating_add(n) > self.itree_nodes.len() {
            return Err(JifError::ITreeNotFound {
                index,
                len: n,
                n_nodes: self.itree_nodes.len(),
            });
        }

        let nodes = self
            .itree_nodes
            .iter()
            .skip(index)
            .take(n)
            .map(|raw| ITreeNode::from_raw(raw, data_map))
            .collect::<Vec<_>>();

        Ok(ITree::new(nodes))
    }
}

impl std::fmt::Debug for Jif {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jif")
            .field("pheaders", &self.pheaders)
            .field("ord", &self.ord_chunks)
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
            .field(
                "data_range",
                &format!(
                    "[{:#x}; {:#x})",
                    self.data_offset,
                    self.data_offset as usize + self.data_segments.len()
                ),
            )
            .finish()
    }
}
