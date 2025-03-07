//! The full JIF representations
//!
//! Includes both the raw and materialized variants

use crate::deduper::{DedupToken, Deduper};
use crate::itree::interval::{AnonIntervalData, LogicalInterval, RawInterval, RefIntervalData};
use crate::itree::interval::{DataSource, IntervalData};
use crate::itree::itree_node::{ITreeNode, IntermediateITreeNode, RawITreeNode};
use crate::itree::ITree;
use crate::ord::OrdChunk;
use crate::pheader::{JifPheader, JifRawPheader};
use crate::utils::{page_align, PAGE_SIZE};
use crate::{error::*, Prot};

use rayon::prelude::*;

use std::cmp::{min, Ordering};
use std::collections::{BTreeMap, HashSet};
use std::io::{BufReader, Read, Seek, Write};
use std::str::from_utf8;
use std::sync::{Arc, RwLock, RwLockReadGuard};

pub(crate) const JIF_MAGIC_HEADER: [u8; 4] = [0x77, b'J', b'I', b'F'];
pub(crate) const JIF_VERSION: u32 = 4;

/// The materialized view over the JIF file
///
/// After materialization the JIF format simplifies greatly:
/// it is simply a list of virtual memory areas (the pheaders)
/// and the ordering list for the prefetcher
pub struct Jif {
    pub(crate) pheaders: Vec<JifPheader>,
    pub(crate) ord_chunks: Vec<OrdChunk>,
    pub(crate) deduper: Arc<RwLock<Deduper>>,
    pub(crate) prefetch: bool,
}

/// The "raw" JIF file representation
/// This consists of a 1:1 mapping into how the data is layed out on disk
///
pub struct JifRaw {
    pub(crate) pheaders: Vec<JifRawPheader>,
    pub(crate) strings_backing: Vec<u8>,
    pub(crate) itree_nodes: Vec<RawITreeNode>,
    pub(crate) ord_chunks: Vec<OrdChunk>,
    pub(crate) n_total_prefetch: u64,
    pub(crate) n_write_prefetch: u64,
    pub(crate) data_offset: u64,
    pub(crate) data_segments: BTreeMap<(u64, u64), Vec<u8>>,
}

#[allow(dead_code)]
#[repr(packed)]
pub struct JifHeaderBinary {
    magic: [u8; 4],
    n_pheaders: u32,
    strings_size: u32,
    itrees_size: u32,
    ord_size: u32,
    version: u32,
    n_write_prefetch: u64,
    n_total_prefetch: u64,
}

impl Jif {
    /// Materialize a [`Jif`] from its raw counterpart
    pub fn from_raw(mut raw: JifRaw) -> JifResult<Self> {
        let data_map = raw.take_data();
        let (deduper, offset_index) = Deduper::from_data_map(data_map);
        let deduper = Arc::new(RwLock::new(deduper));
        let pheaders = raw
            .pheaders
            .par_iter()
            .map(|raw_pheader| {
                JifPheader::from_raw(&raw, raw_pheader, deduper.clone(), &offset_index)
            })
            .collect::<Result<Vec<JifPheader>, _>>()?;

        Ok(Jif {
            pheaders,
            ord_chunks: raw.ord_chunks,
            deduper,
            prefetch: raw.n_total_prefetch > 0,
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
        let header_size = std::mem::size_of::<JifHeaderBinary>();

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
    pub fn build_itrees(&mut self, chroot: Option<std::path::PathBuf>) -> JifResult<()> {
        self.pheaders.par_iter_mut().try_for_each(|pheader| {
            pheader
                .build_itree(&self.deduper.read().unwrap(), &chroot)
                .map_err(|error| JifError::InvalidITree {
                    virtual_range: pheader.virtual_range(),
                    error,
                })
        })
    }

    /// Setup the prefetching section
    /// This includes fracturing ITree intervals by ord chunks
    pub fn setup_prefetch(&mut self) -> JifResult<()> {
        self.prefetch = true;

        self.pheaders.par_iter_mut().try_for_each(|pheader| {
            pheader.fracture_by_ord_chunk(&self.ord_chunks, self.deduper.clone())
        })?;

        let mut tokens_in_use = HashSet::new();
        self.pheaders
            .iter()
            .for_each(|pheader: &JifPheader| match pheader {
                JifPheader::Anonymous { itree, .. } => itree
                    .in_order_intervals()
                    .flat_map(|i| i.data.dedup_token())
                    .for_each(|token| {
                        tokens_in_use.insert(token);
                    }),
                JifPheader::Reference { itree, .. } => itree
                    .in_order_intervals()
                    .flat_map(|i| i.data.dedup_token())
                    .for_each(|token| {
                        tokens_in_use.insert(token);
                    }),
            });

        self.deduper
            .write()
            .unwrap()
            .garbage_collect(&tokens_in_use);

        Ok(())
    }

    /// Fragment vmas based on their source
    pub fn fragment_vmas(&mut self, chroot: Option<std::path::PathBuf>) -> JifResult<()> {
        self.pheaders = self
            .pheaders
            .drain(..)
            .map(|pheader| pheader.fragment_vmas(&self.deduper.read().unwrap(), &chroot))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flat_map(|x| x.into_iter())
            .collect::<Vec<_>>();

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
        // dedup entries with the same address:
        //  - prioritize writes
        //  - keep lowest timestamp
        let mut ords = BTreeMap::new();
        for ord in ordering_info {
            ords.entry(ord.addr())
                .and_modify(|x: &mut OrdChunk| {
                    x.is_written_to |= ord.is_written_to;
                    x.timestamp_us = min(x.timestamp_us, ord.timestamp_us);
                })
                .or_insert(ord);
        }

        ords.iter()
            .find(|(addr, _ord)| self.resolve(**addr).is_none())
            .map_or(Ok(()), |(_addr, ord)| {
                Err(JifError::OrdChunkNotMapped(*ord))
            })?;

        // merge chunks
        let mut chunks = Vec::with_capacity(ords.len());
        let mut mergeable_chunk = OrdChunk::new(0, 0, 0, DataSource::Zero);
        for (_addr, chunk) in ords {
            if !mergeable_chunk.merge_page(self, chunk) {
                // we couldn't merge, push
                assert!(
                    !mergeable_chunk.is_empty(),
                    "trying to add an empty chunk: {mergeable_chunk:?}"
                );
                assert!(
                    self.mapping_pheader_idx(mergeable_chunk.addr()).is_some(),
                    "trying to add a chunk that is not mapped by JIF {mergeable_chunk:?}"
                );
                chunks.push(mergeable_chunk);

                mergeable_chunk = chunk;
            }
        }

        self.ord_chunks = chunks;
        Ok(())
    }

    /// Tag VMAs that belong are referenced by the ordering section
    pub fn tag_vmas(&mut self) {
        // clear tags
        for pheader in self.pheaders.iter_mut() {
            match pheader {
                JifPheader::Anonymous { prot, .. } => *prot &= !(Prot::InOrdering as u8),
                JifPheader::Reference { prot, .. } => *prot &= !(Prot::InOrdering as u8),
            }
        }

        let vaddrs: Vec<_> = self.ord_chunks.iter().map(|x| x.vaddr).collect();
        for vaddr in vaddrs {
            if let Some(pheader) = self.mapping_pheader_mut(vaddr) {
                match pheader {
                    JifPheader::Anonymous { prot, .. } => *prot |= Prot::InOrdering as u8,
                    JifPheader::Reference { prot, .. } => *prot |= Prot::InOrdering as u8,
                }
            }
        }
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

    // Find the pheader that maps a particular address
    pub fn mapping_pheader(&self, vaddr: u64) -> Option<&JifPheader> {
        self.pheaders
            .iter()
            .find(|pheader| pheader.mapps_addr(vaddr))
    }

    // Find the pheader that maps a particular address
    pub fn mapping_pheader_mut(&mut self, vaddr: u64) -> Option<&mut JifPheader> {
        self.pheaders
            .iter_mut()
            .find(|pheader| pheader.mapps_addr(vaddr))
    }

    /// Iterate over all the private pages
    pub fn for_each_private_page<F>(&self, func: F)
    where
        F: FnMut(&[u8]),
    {
        let guard = self.deduper.read().unwrap();
        self.pheaders
            .iter()
            .flat_map(|pheader| pheader.iter_private_pages(&guard))
            .for_each(func);
    }

    /// Iterate over all the shared regions
    pub fn iter_shared_regions(&self) -> impl Iterator<Item = (&str, u64, u64)> {
        self.pheaders
            .iter()
            .flat_map(|phdr| phdr.iter_shared_regions())
    }

    /// Resolve an address into a [`DataSource`]
    pub fn resolve(&self, addr: u64) -> Option<LogicalInterval> {
        self.pheaders
            .iter()
            .find(|phdr| phdr.mapps_addr(addr))
            .map(|phdr| phdr.resolve(addr))
    }

    /// Resolve an address into the private data
    pub fn resolve_data(&self, addr: u64) -> BorrowedData<'_> {
        BorrowedData {
            guard: self.deduper.read().unwrap(),
            pheaders: &self.pheaders,
            addr,
        }
    }

    /// Get the vmas for an ord chunk
    pub fn ord_vma(&self, ord: &OrdChunk) -> Option<&JifPheader> {
        self.mapping_pheader(ord.vaddr)
    }

    /// Get the total of intervals (including implicit) in the Jif
    pub fn n_intervals(&self) -> usize {
        self.pheaders()
            .iter()
            .map(|pheader| pheader.n_intervals())
            .sum()
    }
}

pub struct BorrowedData<'a> {
    guard: RwLockReadGuard<'a, Deduper>,
    pheaders: &'a [JifPheader],
    addr: u64,
}

impl<'a> BorrowedData<'a> {
    pub fn get(&'a self) -> Option<&'a [u8]> {
        self.pheaders
            .iter()
            .find_map(|phdr| phdr.resolve_data(self.addr, &self.guard))
    }
}

impl JifRaw {
    /// Order the data segments keeping in mind the ordering in the ord_chunks
    /// Assumptions:
    ///  - intervals in [`ITree`]s are unique
    ///  - intervals don't overlap
    ///  - ordering chunks span only one interval
    // TODO: simplify the return type, this is a bit too much
    pub(crate) fn order_data_segments(
        itree_nodes: Vec<IntermediateITreeNode>,
        ord_chunks: &[OrdChunk],
        mut data_offset: u64,
    ) -> (
        BTreeMap<DedupToken, (u64, u64)>,
        Vec<RawITreeNode>,
        u64,
        u64,
    ) {
        // Vec of (Interval, <has been serialized>)
        let mut intervals = {
            let mut v = itree_nodes
                .iter()
                .flat_map(|n| n.ranges.iter())
                .map(|ival| (ival, false))
                .collect::<Vec<_>>();
            v.sort_by_key(|(ival, _touched)| ival.start);
            v
        };

        // map from token to range of data offsets in the file
        let mut token_map = BTreeMap::new();
        let mut raw_intervals = BTreeMap::new();
        let mut n_total_prefetch = 0;
        let mut n_write_prefetch = 0;

        for chunk in ord_chunks {
            // if an ordering chunk is not found it is ignored
            if let Ok(idx) = intervals.binary_search_by(|(ival, _)| {
                if ival.start > chunk.vaddr {
                    Ordering::Greater
                } else if ival.end <= chunk.vaddr {
                    Ordering::Less
                } else {
                    Ordering::Equal
                }
            }) {
                // if we already serialized this, we can continue
                if intervals[idx].1 {
                    continue;
                }

                intervals[idx].1 = true;

                let new_interval = RawInterval::from_intermediate(
                    intervals[idx].0,
                    &mut token_map,
                    &mut data_offset,
                );

                raw_intervals.insert((new_interval.start, new_interval.end), new_interval);

                let chunk_size = (new_interval.end - new_interval.start) / PAGE_SIZE as u64;
                n_total_prefetch += chunk_size;
                if chunk.is_written_to {
                    n_write_prefetch += chunk_size;
                }
            }
        }

        for inter in intervals.iter_mut().filter(|(_ival, touched)| !touched) {
            let new_interval =
                RawInterval::from_intermediate(inter.0, &mut token_map, &mut data_offset);

            raw_intervals.insert((new_interval.start, new_interval.end), new_interval);
        }

        let raw_itree_nodes = itree_nodes
            .into_iter()
            .map(|itree_node| RawITreeNode::from_intermediate(itree_node, &mut raw_intervals))
            .collect();

        (
            token_map,
            raw_itree_nodes,
            n_total_prefetch,
            n_write_prefetch,
        )
    }

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

        let mut itree_nodes = Vec::new();
        let data_offset = jif.data_offset();
        let pheaders = jif
            .pheaders
            .iter()
            .map(|phdr| {
                JifRawPheader::from_materialized(
                    phdr,
                    &string_map,
                    &mut itree_nodes,
                    &mut jif.deduper.write().unwrap(),
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

        let mut ord_chunks: Vec<OrdChunk> = jif
            .ord_chunks
            .iter()
            .flat_map(|x| x.split_by_intervals(&jif).into_iter())
            .collect();

        // Data is layed out differently depending on whether prefetch is set up or not
        // If prefetch is not set up, both ordering chunks and data are ordered by timestamp
        // If prefetch is set up, we partition the private data in two partitions
        //  - the first partition has the pages that are written to
        //  - the second partition has the pages that are not written to
        //  Both partitions are ordered by address internally
        //  The ordering section, however, is totally ordered by address only

        // define data ordering
        if jif.prefetch {
            ord_chunks.sort_by_key(|c| c.addr());
            ord_chunks.sort_by_key(|c| if c.is_written_to { 0 } else { 1 });
        } else {
            ord_chunks.sort_by_key(|c| c.timestamp_us);
        }

        let (token_map, itree_nodes, n_prefetch, n_write_prefetch) =
            Self::order_data_segments(itree_nodes, &ord_chunks, data_offset);
        let data_segments = jif.deduper.write().unwrap().destructure(token_map);

        // After the jif data has been sorted into the read/write partitions, we re-order the
        // ordering chunks by address.
        // This allows for a single VMA lookup on prefetch (per VMA)
        if jif.prefetch {
            ord_chunks.sort_by_key(|c| c.addr());
        }

        // clamp n_prefetch if prefetching has not been set up
        let n_total_prefetch = if jif.prefetch { n_prefetch } else { 0 };

        JifRaw {
            pheaders,
            strings_backing,
            itree_nodes,
            ord_chunks,
            data_offset,
            data_segments,
            n_total_prefetch,
            n_write_prefetch,
        }
    }

    /// Construct a raw JIF from a materialized one
    pub fn from_materialized_ref(jif: &mut Jif) -> Self {
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

        let mut itree_nodes = Vec::new();
        let data_offset = jif.data_offset();
        let pheaders = jif
            .pheaders
            .iter()
            .map(|phdr| {
                JifRawPheader::from_materialized(
                    phdr,
                    &string_map,
                    &mut itree_nodes,
                    &mut jif.deduper.write().unwrap(),
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

        // Sort chunks by kind.
        jif.ord_chunks.sort_by_key(|c| match c.kind {
            DataSource::Zero => 1,
            DataSource::Shared => 2,
            DataSource::Private => 0,
        });

        let (token_map, itree_nodes, n_prefetch, n_write_prefetch) =
            Self::order_data_segments(itree_nodes, &jif.ord_chunks, data_offset);
        let data_segments = jif.deduper.read().unwrap().clone_segments(token_map);

        // clamp n_prefetch if prefetching has not been set up
        let n_total_prefetch = if jif.prefetch { n_prefetch } else { 0 };

        JifRaw {
            pheaders,
            strings_backing,
            itree_nodes,
            ord_chunks: jif.ord_chunks.clone(),
            data_offset,
            data_segments,
            n_total_prefetch,
            n_write_prefetch,
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

    /// Data offset
    pub fn data_offset(&self) -> usize {
        self.data_offset as usize
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
        deduper: Arc<RwLock<Deduper>>,
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
                ITreeNode::from_raw_anon(
                    raw,
                    self.data_offset,
                    &deduper.read().unwrap(),
                    offset_idx,
                )
                .map_err(|itree_node_err| JifError::BadITreeNode {
                    itree_node_idx,
                    itree_node_err,
                })
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
        deduper: Arc<RwLock<Deduper>>,
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
            .map(|raw| {
                ITreeNode::from_raw_ref(raw, self.data_offset, &deduper.read().unwrap(), offset_idx)
            })
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
            .field("prefetch", &self.prefetch)
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
            .field("n_write_prefetch", &self.n_write_prefetch)
            .field("n_total_prefetch", &self.n_total_prefetch)
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

    use crate::itree::interval::{IntermediateInterval, IntermediateIntervalData};
    use crate::pheader::test::gen_pheader;
    pub(crate) fn gen_jif(vaddrs: &[((u64, u64), &[(u64, u64)])]) -> Jif {
        Jif {
            pheaders: vaddrs
                .into_iter()
                .map(|(range, ivals)| gen_pheader(*range, ivals))
                .collect(),
            ord_chunks: vec![],
            deduper: Arc::new(RwLock::new(Deduper::default())),
            prefetch: false,
        }
    }

    #[test]
    fn test_order_segments_empty() {
        let (token_map, itree_nodes, _n_prefetch, _n_write_prefetch) =
            JifRaw::order_data_segments(vec![], &[], 0);
        assert!(token_map.is_empty());
        assert!(itree_nodes.is_empty());
    }

    #[test]
    fn test_order_segments() {
        fn inter_node(ival: IntermediateInterval) -> IntermediateITreeNode {
            let mut node = IntermediateITreeNode::default();
            node.ranges[0] = ival;
            node
        }
        // 1: dedup some segments and create some intermediate itree nodes
        let deduper = RwLock::new(Deduper::default());
        let mut intermediate_nodes = Vec::new();
        intermediate_nodes.push(inter_node(IntermediateInterval {
            start: 0x1000,
            end: 0x2000,
            data: IntermediateIntervalData::Zero,
        }));

        let token1 = deduper.write().unwrap().insert(vec![42; 0x2000]);
        intermediate_nodes.push(inter_node(IntermediateInterval {
            start: 0x3000,
            end: 0x5000,
            data: IntermediateIntervalData::Ref(token1),
        }));

        let token2 = deduper.write().unwrap().insert(vec![42; 0x2000]);
        assert_eq!(token1, token2);
        intermediate_nodes.push(inter_node(IntermediateInterval {
            start: 0x6000,
            end: 0x8000,
            data: IntermediateIntervalData::Ref(token2),
        }));

        intermediate_nodes.push(inter_node(IntermediateInterval {
            start: 0x8000,
            end: 0x9000,
            data: IntermediateIntervalData::Zero,
        }));

        let token3 = deduper.write().unwrap().insert(vec![84; 0x1000]);
        intermediate_nodes.push(inter_node(IntermediateInterval {
            start: 0x10000,
            end: 0x11000,
            data: IntermediateIntervalData::Ref(token3),
        }));

        // 2: create some ordering segments (make sure they aren't bad)
        let ord_chunks = [
            OrdChunk {
                timestamp_us: 0,
                vaddr: 0x10000,
                n_pages: 1,
                kind: DataSource::Zero,
                is_written_to: false,
            },
            OrdChunk {
                timestamp_us: 0,
                vaddr: 0x7000,
                n_pages: 1,
                kind: DataSource::Zero,
                is_written_to: false,
            },
            OrdChunk {
                timestamp_us: 0,
                vaddr: 0x8000,
                n_pages: 1,
                kind: DataSource::Zero,
                is_written_to: false,
            },
            OrdChunk {
                timestamp_us: 0,
                vaddr: 0x6000,
                n_pages: 1,
                kind: DataSource::Zero,
                is_written_to: false,
            },
            OrdChunk {
                timestamp_us: 0,
                vaddr: 0x3000,
                n_pages: 2,
                kind: DataSource::Zero,
                is_written_to: false,
            },
            OrdChunk {
                timestamp_us: 0,
                vaddr: 0x1000,
                n_pages: 1,
                kind: DataSource::Zero,
                is_written_to: false,
            },
        ];

        // 3: call order_data_segments
        let (token_map, itree_nodes, _n_total_prefetch, _n_write_prefetch) =
            JifRaw::order_data_segments(intermediate_nodes, &ord_chunks, 0);

        // 4: check order
        assert_eq!(token_map.get(&token1), Some(&(0x1000, 0x3000)));
        assert_eq!(token_map.get(&token3), Some(&(0x0000, 0x1000)));

        // 5: check intervals
        let intervals = {
            let mut ivals = itree_nodes
                .into_iter()
                .flat_map(|node| node.ranges.into_iter())
                .filter(|ival| ival.start != u64::MAX && ival.end != u64::MAX)
                .collect::<Vec<_>>();
            ivals.sort_by_key(|ival| ival.start);
            ivals
        };
        assert_eq!(
            intervals,
            vec![
                RawInterval {
                    start: 0x1000,
                    end: 0x2000,
                    offset: u64::MAX
                },
                RawInterval {
                    start: 0x3000,
                    end: 0x5000,
                    offset: 0x1000
                },
                RawInterval {
                    start: 0x6000,
                    end: 0x8000,
                    offset: 0x1000
                },
                RawInterval {
                    start: 0x8000,
                    end: 0x9000,
                    offset: u64::MAX
                },
                RawInterval {
                    start: 0x10000,
                    end: 0x11000,
                    offset: 0x0000
                },
            ]
        );
    }
}
