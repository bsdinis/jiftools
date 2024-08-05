//! The full JIF representations
//!
//! Includes both the raw and materialized variants

use crate::deduper::{DedupToken, Deduper};
use crate::error::*;
use crate::itree::interval::{AnonIntervalData, LogicalInterval, RefIntervalData};
use crate::itree::itree_node::{ITreeNode, RawITreeNode};
use crate::itree::ITree;
use crate::ord::OrdChunk;
use crate::pheader::{JifPheader, JifRawPheader};
use crate::utils::page_align;

use std::collections::{BTreeMap, HashSet};
use std::io::{BufReader, Read, Seek, Write};
use std::str::from_utf8;
use std::u64;

pub(crate) const JIF_MAGIC_HEADER: [u8; 4] = [0x77, b'J', b'I', b'F'];

/// The materialized view over the JIF file
///
/// After materialization the JIF format simplifies greatly:
/// it is simply a list of virtual memory areas (the pheaders)
/// and the ordering list for the prefetcher
pub struct Jif {
    pub(crate) pheaders: Vec<JifPheader>,
    pub(crate) ord_chunks: Vec<OrdChunk>,
    pub(crate) deduper: Deduper,
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
    pub(crate) data_segments: BTreeMap<(u64, u64), Vec<u8>>,
}

impl Jif {
    /// Materialize a [`Jif`] from its raw counterpart
    pub fn from_raw(mut raw: JifRaw) -> JifResult<Self> {
        let data_map = raw.take_data();
        let (deduper, offset_index) = Deduper::from_data_map(data_map);
        let pheaders = raw
            .pheaders
            .iter()
            .map(|raw_pheader| JifPheader::from_raw(&raw, raw_pheader, &deduper, &offset_index))
            .collect::<Result<Vec<JifPheader>, _>>()?;

        Ok(Jif {
            pheaders,
            ord_chunks: raw.ord_chunks,
            deduper,
        })
    }

    /// List out all the strings in the pheaders
    pub fn strings(&self) -> HashSet<&str> {
        self.pheaders
            .iter()
            .filter_map(|phdr| match phdr {
                JifPheader::Anonymous { .. } => None,
                JifPheader::Reference { ref_path, .. } => Some(ref_path.as_str()),
            })
            .collect()
    }

    /// Read the [`Jif`] from a file
    pub fn from_reader<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<Self> {
        Jif::from_raw(JifRaw::from_reader(r)?)
    }

    /// Write the [`Jif`] to a file
    pub fn to_writer<W: Write>(self, w: &mut W) -> std::io::Result<usize> {
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
            .map(|phdr| match phdr {
                JifPheader::Anonymous { itree, .. } => itree.n_nodes(),
                JifPheader::Reference { itree, .. } => itree.n_nodes(),
            })
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
        for pheader in self.pheaders.iter_mut() {
            pheader
                .build_itree(&self.deduper)
                .map_err(|error| JifError::InvalidITree {
                    virtual_range: pheader.virtual_range(),
                    error,
                })?;
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
                    // we ignore unmapped addresses
                    // there might've been a recorded chunk that was only mapped after
                    // the snapshot point, which means it will be mapped after the restore is
                    // complete (i.e., there is no way to prefetch)
                    if chunk.is_empty() || jif.mapping_pheader_idx(chunk.vaddr).is_none() {
                        None
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

    /// Compute the total number of zero pages encoded (by omission) in the [`Jif`]
    pub fn zero_pages(&self) -> usize {
        self.pheaders.iter().map(|phdr| phdr.zero_pages()).sum()
    }

    /// Compute the total number of private pages stored (directly) in the [`Jif`]
    pub fn private_pages(&self) -> usize {
        self.pheaders.iter().map(|phdr| phdr.private_pages()).sum()
    }

    /// Compute the total number of shared pages referenced by the [`Jif`]
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

    // Find the pheader (by index) that maps a particular address
    pub fn mapping_pheader(&self, vaddr: u64) -> Option<&JifPheader> {
        self.pheaders
            .iter()
            .find(|pheader| pheader.mapps_addr(vaddr))
    }

    /// Iterate over all the private pages
    pub fn iter_private_pages(&self) -> impl Iterator<Item = &[u8]> {
        self.pheaders
            .iter()
            .flat_map(|phdr| phdr.iter_private_pages(&self.deduper))
    }

    /// Resolve an address into a [`DataSource`]
    pub fn resolve(&self, addr: u64) -> Option<LogicalInterval> {
        self.pheaders
            .iter()
            .find(|phdr| phdr.mapps_addr(addr))
            .map(|phdr| phdr.resolve(addr))
    }
}

impl JifRaw {
    /// Construct a raw JIF from a materialized one
    pub fn from_materialized(mut jif: Jif) -> Self {
        // print pheaders in order
        jif.pheaders.sort_by_key(|phdr| phdr.virtual_range().0);

        let string_map = {
            let strings = jif
                .strings()
                .into_iter()
                .map(|s| s.to_string())
                .collect::<HashSet<String>>();

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
        let mut last_data_offset = jif.data_offset();
        let mut itree_nodes = Vec::new();
        let mut token_map = BTreeMap::new();
        let pheaders = jif
            .pheaders
            .into_iter()
            .map(|phdr| {
                JifRawPheader::from_materialized(
                    phdr,
                    &string_map,
                    &mut itree_nodes,
                    &mut jif.deduper,
                    &mut token_map,
                    &mut last_data_offset,
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

        let data_segments = jif.deduper.destructure(token_map);

        JifRaw {
            pheaders,
            strings_backing,
            itree_nodes,
            ord_chunks: jif.ord_chunks,
            data_offset,
            data_segments,
        }
    }

    /// Remove the data from the [`JifRaw`]
    pub fn take_data(&mut self) -> BTreeMap<(u64, u64), Vec<u8>> {
        self.data_segments.split_off(&(0, 0))
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

    /// Report the number of stored bytes
    pub fn data_size(&self) -> usize {
        self.data_segments.values().map(Vec::len).sum()
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

    /// Get an anonymous interval tree from an (index, len) range
    pub(crate) fn get_anon_itree(
        &self,
        index: usize,
        n: usize,
        virtual_range: (u64, u64),
        deduper: &Deduper,
        offset_idx: &BTreeMap<(u64, u64), DedupToken>,
    ) -> JifResult<ITree<AnonIntervalData>> {
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
            .enumerate()
            .skip(index)
            .take(n)
            .map(|(itree_node_idx, raw)| {
                ITreeNode::from_raw_anon(raw, self.data_offset, deduper, offset_idx).map_err(
                    |itree_node_err| JifError::BadITreeNode {
                        itree_node_idx,
                        itree_node_err,
                    },
                )
            })
            .collect::<JifResult<Vec<_>>>()?;

        ITree::new(nodes, virtual_range).map_err(|error| JifError::InvalidITree {
            virtual_range,
            error,
        })
    }

    /// Get a reference interval tree from an (index, len) range
    pub(crate) fn get_ref_itree(
        &self,
        index: usize,
        n: usize,
        virtual_range: (u64, u64),
        deduper: &Deduper,
        offset_idx: &BTreeMap<(u64, u64), DedupToken>,
    ) -> JifResult<ITree<RefIntervalData>> {
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
            .map(|raw| ITreeNode::from_raw_ref(raw, self.data_offset, deduper, offset_idx))
            .collect::<Vec<_>>();

        ITree::new(nodes, virtual_range).map_err(|error| JifError::InvalidITree {
            virtual_range,
            error,
        })
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
                    self.data_offset as usize + self.data_size()
                ),
            )
            .finish()
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::pheader::test::gen_pheader;
    pub(crate) fn gen_jif(vaddrs: &[(u64, u64)]) -> Jif {
        Jif {
            pheaders: vaddrs
                .into_iter()
                .map(|range| gen_pheader(*range))
                .collect(),
            ord_chunks: vec![],
            deduper: Deduper::default(),
        }
    }
}
