//! The pheader representation

use std::collections::{BTreeMap, HashSet};

use crate::deduper::{DedupToken, Deduper};
use crate::error::*;
use crate::itree::diff::{
    create_anon_itree_from_zero_page, create_itree_from_diff, create_ref_itree_from_zero_page,
};
use crate::itree::interval::{
    AnonIntervalData, Interval, IntervalData, LogicalInterval, RefIntervalData,
};
use crate::itree::itree_node::IntermediateITreeNode;
use crate::itree::{ITree, ITreeView};
use crate::jif::JifRaw;
use crate::ord::OrdChunk;
use crate::utils::{page_align, PAGE_SIZE};

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::PathBuf;

/// VMA protection bits
#[repr(u8)]
pub enum Prot {
    Read = 1u8 << 2,
    Write = 1u8 << 1,
    Exec = 1u8 << 0,
}

/// A materialized JIF pheader
///
/// There are two types of pheaders: anonymous and reference.
///
/// All pheaders include an address range, protections and an interval tree
///
/// Anonymous pheaders refer to anonymous memory.
/// The interval tree can only hold data.
/// Failing to resolve means it should be backed by the zero page.
///
/// Reference pheaders have a file that is backing all the segment.
/// As such, they also hold the information regarding said file.
/// Moreover, the interval tree intervals can refer either to data or to the zero page.
/// Failing to resolve means it should be backed by the underlying file mapping.
///
/// Can be used to visualize the VMA and manipulate it (e.g., construct an interal tree)
pub enum JifPheader {
    Anonymous {
        /// virtual address range
        vaddr_range: (u64, u64),
        /// interval tree
        itree: ITree<AnonIntervalData>,

        /// VMA protections
        prot: u8,
    },
    Reference {
        /// virtual address range
        vaddr_range: (u64, u64),
        /// interval tree
        itree: ITree<RefIntervalData>,

        /// VMA protections
        prot: u8,

        /// reference path
        ref_path: String,

        /// reference into the file
        ref_offset: u64,
    },
}

/// The "raw" JIF pheader
///
/// This type encodes 1:1 the information as it is serialized in the JIF format
/// It can be used to construct materialized pheaders with the help of the raw [`JifRaw`] type.
pub struct JifRawPheader {
    pub(crate) vbegin: u64,
    pub(crate) vend: u64,

    pub(crate) ref_offset: u64,

    pub(crate) itree_idx: u32,
    pub(crate) itree_n_nodes: u32,

    pub(crate) pathname_offset: u32,

    pub(crate) prot: u8,
}

impl JifPheader {
    /// Construct a materialized JIF pheader from its raw counterpart
    pub(crate) fn from_raw(
        jif: &JifRaw,
        raw: &JifRawPheader,
        deduper: &Deduper,
        offset_idx: &BTreeMap<(u64, u64), DedupToken>,
    ) -> JifResult<Self> {
        let vaddr_range = (raw.vbegin, raw.vend);

        let ref_segment = jif
            .string_at_offset(raw.pathname_offset as usize)
            .map(|s| (s.to_string(), raw.ref_offset));

        if let Some((ref_path, ref_offset)) = ref_segment {
            let itree = jif.get_ref_itree(
                raw.itree_idx as usize,
                raw.itree_n_nodes as usize,
                raw.virtual_range(),
                deduper,
                offset_idx,
            )?;

            Ok(JifPheader::Reference {
                vaddr_range,
                ref_path,
                ref_offset,
                itree,
                prot: raw.prot,
            })
        } else {
            let itree = jif.get_anon_itree(
                raw.itree_idx as usize,
                raw.itree_n_nodes as usize,
                raw.virtual_range(),
                deduper,
                offset_idx,
            )?;

            Ok(JifPheader::Anonymous {
                vaddr_range,
                itree,
                prot: raw.prot,
            })
        }
    }

    /// Build an itree for a particular pheader
    pub fn build_itree(
        &mut self,
        deduper: &Deduper,
        chroot: &Option<std::path::PathBuf>,
    ) -> ITreeResult<()> {
        fn build_anon_from_zero(
            itree: &mut ITree<AnonIntervalData>,
            virtual_range: (u64, u64),
            deduper: &Deduper,
        ) -> ITreeResult<()> {
            let orig_itree = itree.take();
            let mut intervals = vec![];
            let data_intervals: Vec<Interval<AnonIntervalData>> = orig_itree
                .into_iter_intervals()
                .filter(|i| i.is_data())
                .collect();

            for data_interval in data_intervals {
                let ival_len = data_interval.len() as usize;
                if let Some(data) = data_interval.data.get_data(deduper) {
                    assert_eq!(data.len(), ival_len);
                    create_anon_itree_from_zero_page(data, data_interval.start, &mut intervals)
                } else {
                    panic!("we checked that this was an interval with data but there was no data");
                }
            }

            *itree = ITree::build(intervals, virtual_range)?;
            Ok(())
        }

        fn build_from_diff(
            overlay: &[u8],
            virtual_range: (u64, u64),
            refs: &str,
            ref_offset: u64,
            chroot: &Option<std::path::PathBuf>,
        ) -> ITreeResult<ITree<RefIntervalData>> {
            let mut file = {
                let ref_path = PathBuf::from(refs);
                let full_path = match chroot {
                    None => ref_path,
                    Some(cpath) => {
                        let mut cp = cpath.clone();
                        if ref_path.is_absolute() {
                            cp.push(ref_path.iter().skip(1).collect::<std::path::PathBuf>());
                        } else {
                            cp.push(ref_path);
                        }
                        cp
                    }
                };
                let mut f = BufReader::new(File::open(&full_path)?);
                f.seek(SeekFrom::Start(ref_offset))?;
                f
            };

            let base = {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)?;

                let delta_to_page = page_align(buf.len() as u64) as usize - buf.len();
                if delta_to_page > 0 {
                    buf.extend(std::iter::repeat(0x00u8).take(delta_to_page));
                }
                buf
            };

            let mut intervals = Vec::new();
            create_itree_from_diff(&base, overlay, virtual_range.0, &mut intervals);
            ITree::build(intervals, virtual_range)
        }
        fn build_ref_from_zero(
            itree: &mut ITree<RefIntervalData>,
            virtual_range: (u64, u64),
            deduper: &Deduper,
        ) -> ITreeResult<()> {
            let orig_itree = itree.take();
            let mut intervals = orig_itree
                .in_order_intervals()
                .filter(|i| i.is_zero())
                .cloned()
                .collect();
            let data_intervals: Vec<Interval<RefIntervalData>> = orig_itree
                .into_iter_intervals()
                .filter(|i| i.is_data())
                .collect();

            for data_interval in data_intervals {
                let ival_len = data_interval.len() as usize;
                if let Some(data) = data_interval.data.get_data(deduper) {
                    assert_eq!(data.len(), ival_len);
                    create_ref_itree_from_zero_page(data, data_interval.start, &mut intervals)
                } else {
                    panic!("we checked that this was an interval with data but there was no data");
                }
            }

            *itree = ITree::build(intervals, virtual_range)?;
            Ok(())
        }

        match self {
            JifPheader::Reference {
                itree,
                ref_path,
                ref_offset,
                vaddr_range,
                ..
            } => {
                if itree.n_data_intervals() != 1 {
                    build_ref_from_zero(itree, *vaddr_range, deduper)?
                } else {
                    let data_interval = itree
                        .in_order_intervals()
                        .find(|i| i.is_data())
                        .expect("we checked there was a data interval");

                    if data_interval.start != vaddr_range.0 {
                        build_ref_from_zero(itree, *vaddr_range, deduper)?
                    } else if let Some(overlay) = data_interval.data.get_data(deduper) {
                        *itree =
                            build_from_diff(overlay, *vaddr_range, ref_path, *ref_offset, chroot)?;
                    } else {
                        panic!("we checked this was a data interval but there was no data");
                    }
                }
            }
            JifPheader::Anonymous {
                itree, vaddr_range, ..
            } => {
                build_anon_from_zero(itree, *vaddr_range, deduper)?;
            }
        }

        Ok(())
    }

    /// Fragment pheader based on data source
    pub fn fragment_vmas(
        mut self,
        deduper: &Deduper,
        chroot: &Option<std::path::PathBuf>,
    ) -> JifResult<Vec<JifPheader>> {
        self.build_itree(deduper, chroot)
            .map_err(|error| JifError::InvalidITree {
                virtual_range: self.virtual_range(),
                error,
            })?;

        Ok(match self {
            JifPheader::Anonymous {
                vaddr_range,
                itree,
                prot,
            } => std::iter::once((0, vaddr_range.0, None))
                .chain(
                    itree
                        .in_order_intervals()
                        .map(|ival| (ival.start, ival.end, Some(&ival.data))),
                )
                .zip(
                    itree
                        .in_order_intervals()
                        .map(|ival| ival.start)
                        .chain(std::iter::once(vaddr_range.1)),
                )
                .flat_map(|((s1, e1, d1), s2)| {
                    let first_iter: Box<dyn Iterator<Item = JifPheader>> = if let Some(data) = d1 {
                        Box::new(std::iter::once(JifPheader::Anonymous {
                            vaddr_range: (s1, e1),
                            itree: ITree::single((s1, e1), data.clone()),
                            prot,
                        }))
                    } else {
                        Box::new(std::iter::empty())
                    };
                    let second_iter: Box<dyn Iterator<Item = JifPheader>> = if e1 < s2 {
                        Box::new(std::iter::once(JifPheader::Anonymous {
                            vaddr_range: (e1, s2),
                            itree: ITree::single_default((e1, s2)),
                            prot,
                        }))
                    } else {
                        Box::new(std::iter::empty())
                    };

                    first_iter.chain(second_iter)
                })
                .collect(),
            JifPheader::Reference {
                vaddr_range,
                itree,
                prot,
                ref_path,
                ref_offset,
            } => std::iter::once((0, vaddr_range.0, None))
                .chain(
                    itree
                        .in_order_intervals()
                        .map(|ival| (ival.start, ival.end, Some(&ival.data))),
                )
                .zip(
                    itree
                        .in_order_intervals()
                        .map(|ival| ival.start)
                        .chain(std::iter::once(vaddr_range.1)),
                )
                .flat_map(|((s1, e1, d1), s2)| {
                    let first_iter: Box<dyn Iterator<Item = JifPheader>> = match d1 {
                        Some(data) if data.is_data() => {
                            Box::new(std::iter::once(JifPheader::Anonymous {
                                vaddr_range: (s1, e1),
                                itree: ITree::single(
                                    (s1, e1),
                                    data.clone()
                                        .try_into()
                                        .expect("we checked it wasn't a reference section"),
                                ),
                                prot,
                            }))
                        }
                        Some(data) if data.is_zero() => {
                            Box::new(std::iter::once(JifPheader::Anonymous {
                                vaddr_range: (s1, e1),
                                itree: ITree::single_default((s1, e1)),
                                prot,
                            }))
                        }
                        Some(_data) => Box::new(std::iter::once(JifPheader::Reference {
                            vaddr_range: (s1, e1),
                            itree: ITree::single_default((s1, e1)),
                            ref_path: ref_path.clone(),
                            ref_offset: ref_offset + (s1 - vaddr_range.0),
                            prot,
                        })),
                        None => Box::new(std::iter::empty()),
                    };

                    let second_iter: Box<dyn Iterator<Item = JifPheader>> = if e1 < s2 {
                        Box::new(std::iter::once(JifPheader::Reference {
                            vaddr_range: (e1, s2),
                            itree: ITree::single_default((e1, s2)),
                            ref_path: ref_path.clone(),
                            ref_offset: ref_offset + (e1 - vaddr_range.0),
                            prot,
                        }))
                    } else {
                        Box::new(std::iter::empty())
                    };

                    first_iter.chain(second_iter)
                })
                .collect(),
        })
    }

    /// Fracture the pheader intervals based on the ordering chunks that it backs.
    /// This allows the ordering chunks to be reordered
    pub fn fracture_by_ord_chunk(
        &mut self,
        ord_chunks: &[OrdChunk],
        deduper: &Deduper,
    ) -> JifResult<()> {
        match self {
            JifPheader::Anonymous { itree, .. } => itree.fracture(ord_chunks, deduper),
            JifPheader::Reference { itree, .. } => itree.fracture(ord_chunks, deduper),
        }
    }

    /// Absorb owned data pieces into the deduper
    pub fn dedup(&mut self, deduper: &mut Deduper) {
        match self {
            JifPheader::Anonymous { itree, .. } => itree.dedup(deduper),
            JifPheader::Reference { itree, .. } => itree.dedup(deduper),
        }
    }

    /// Collect all tokens in use
    pub fn add_tokens_in_use(&self, tokens_in_use: &mut HashSet<DedupToken>) {
        match self {
            JifPheader::Anonymous { itree, .. } => itree.add_tokens_in_use(tokens_in_use),
            JifPheader::Reference { itree, .. } => itree.add_tokens_in_use(tokens_in_use),
        }
    }

    /// Rename the file in this pheader if 1) it has a file and 2) it matches the name
    pub fn rename_file(&mut self, old: &str, new: &str) {
        if let JifPheader::Reference { ref_path, .. } = self {
            if ref_path == old {
                *ref_path = new.to_string();
            }
        }
    }

    /// Check whether this pheader maps a particular address
    pub(crate) fn mapps_addr(&self, addr: u64) -> bool {
        self.virtual_range().0 <= addr && addr < self.virtual_range().1
    }

    /// Resolve an address
    pub(crate) fn resolve(&self, addr: u64) -> LogicalInterval {
        self.itree().resolve(addr)
    }

    /// Resolve an address into a private data page
    pub(crate) fn resolve_data<'a>(&'a self, addr: u64, deduper: &'a Deduper) -> Option<&'a [u8]> {
        self.itree().resolve_data(addr, deduper)
    }

    /// The virtual address space range that this pheader maps
    pub fn virtual_range(&self) -> (u64, u64) {
        match self {
            JifPheader::Anonymous { vaddr_range, .. } => *vaddr_range,
            JifPheader::Reference { vaddr_range, .. } => *vaddr_range,
        }
    }

    /// A view over the underlying [`ITree`]
    pub fn itree(&self) -> ITreeView {
        match self {
            JifPheader::Anonymous { itree, .. } => ITreeView::Anon { inner: itree },
            JifPheader::Reference { itree, .. } => ITreeView::Ref { inner: itree },
        }
    }

    /// The size of the [`ITree`] in number of nodes
    pub fn n_itree_nodes(&self) -> usize {
        self.itree().n_nodes()
    }

    /// The pathname of the reference section
    pub fn pathname(&self) -> Option<&str> {
        match self {
            JifPheader::Anonymous { .. } => None,
            JifPheader::Reference { ref_path, .. } => Some(ref_path.as_str()),
        }
    }

    /// The offset range into the referenced file which is used to map the file data into this vma
    pub fn ref_offset(&self) -> Option<u64> {
        match self {
            JifPheader::Anonymous { .. } => None,
            JifPheader::Reference { ref_offset, .. } => Some(*ref_offset),
        }
    }

    /// The protections concerning this vma
    pub fn prot(&self) -> u8 {
        match self {
            JifPheader::Anonymous { prot, .. } => *prot,
            JifPheader::Reference { prot, .. } => *prot,
        }
    }

    /// Size of the stored data (in Bytes)
    pub fn data_size(&self) -> usize {
        self.itree().private_data_size()
    }

    /// Number of zero pages encoded (by ommission) in this pheader
    pub fn zero_pages(&self) -> usize {
        (match self {
            JifPheader::Anonymous {
                itree, vaddr_range, ..
            } => itree.implicitely_mapped_subregion_size(vaddr_range.0, vaddr_range.1),
            JifPheader::Reference { itree, .. } => itree.zero_byte_size(),
        }) / PAGE_SIZE
    }

    /// Number of private data pages in this pheader
    pub fn private_pages(&self) -> usize {
        self.data_size() / PAGE_SIZE
    }

    /// Number of pages coming from the reference file
    pub fn shared_pages(&self) -> usize {
        (match self {
            JifPheader::Anonymous { .. } => 0,
            JifPheader::Reference {
                itree, vaddr_range, ..
            } => itree.implicitely_mapped_subregion_size(vaddr_range.0, vaddr_range.1),
        }) / PAGE_SIZE
    }

    /// Total number of pages in the pheader
    pub fn total_pages(&self) -> usize {
        let (begin, end) = self.virtual_range();

        assert_eq!(
            (end as usize - begin as usize) / PAGE_SIZE,
            self.zero_pages() + self.private_pages() + self.shared_pages()
        );
        (end as usize - begin as usize) / PAGE_SIZE
    }

    /// Iterate over the private pages in the pheader
    pub(crate) fn iter_private_pages<'a>(
        &'a self,
        deduper: &'a Deduper,
    ) -> Box<dyn Iterator<Item = &'a [u8]> + 'a> {
        match self {
            JifPheader::Anonymous { itree, .. } => Box::new(itree.iter_private_pages(deduper)),
            JifPheader::Reference { itree, .. } => Box::new(itree.iter_private_pages(deduper)),
        }
    }

    /// Iterate over the private pages in the pheader
    pub(crate) fn iter_shared_regions<'a>(
        &'a self,
    ) -> Box<dyn Iterator<Item = (&'a str, u64, u64)> + 'a> {
        match self {
            JifPheader::Anonymous { .. } => Box::new(std::iter::empty()),
            JifPheader::Reference {
                itree,
                ref_path,
                ref_offset,
                vaddr_range,
                ..
            } => Box::new(itree.iter_unmapped_regions().map(|(start, end)| {
                (
                    ref_path.as_str(),
                    start - vaddr_range.0 + *ref_offset,
                    end - vaddr_range.0 + *ref_offset,
                )
            })),
        }
    }
}

impl JifRawPheader {
    /// Serialized size of the raw JIF Pheader
    pub const fn serialized_size() -> usize {
        3 * std::mem::size_of::<u64>() + 3 * std::mem::size_of::<u32>() + std::mem::size_of::<u8>()
    }

    /// Reconstruct the pheader from its materialized counterpart
    pub(crate) fn from_materialized(
        jif: JifPheader,
        string_map: &BTreeMap<String, usize>,
        itree_nodes: &mut Vec<IntermediateITreeNode>,
        deduper: &mut Deduper,
    ) -> JifRawPheader {
        match jif {
            JifPheader::Anonymous {
                vaddr_range,
                itree,
                prot,
            } => {
                let (vbegin, vend) = vaddr_range;
                let (itree_idx, itree_n_nodes) = {
                    let idx = itree_nodes.len() as u32;
                    let len = itree.nodes.len() as u32;

                    itree_nodes.reserve(itree.nodes.len());
                    for node in itree.nodes {
                        let new_node = IntermediateITreeNode::from_materialized_anon(node, deduper);
                        itree_nodes.push(new_node)
                    }

                    (idx, len)
                };

                JifRawPheader {
                    vbegin,
                    vend,
                    ref_offset: u64::MAX,
                    itree_idx,
                    itree_n_nodes,
                    pathname_offset: u32::MAX,
                    prot,
                }
            }
            JifPheader::Reference {
                vaddr_range,
                itree,
                prot,
                ref_path,
                ref_offset,
            } => {
                let (vbegin, vend) = vaddr_range;
                let (itree_idx, itree_n_nodes) = {
                    let idx = itree_nodes.len() as u32;
                    let len = itree.nodes.len() as u32;

                    itree_nodes.reserve(itree.nodes.len());
                    for node in itree.nodes {
                        let new_node = IntermediateITreeNode::from_materialized_ref(node, deduper);
                        itree_nodes.push(new_node)
                    }

                    (idx, len)
                };
                let pathname_offset = string_map
                    .get(&ref_path)
                    .map(|path_offset| *path_offset as u32)
                    .unwrap_or(u32::MAX);

                JifRawPheader {
                    vbegin,
                    vend,
                    ref_offset,
                    itree_idx,
                    itree_n_nodes,
                    pathname_offset,
                    prot,
                }
            }
        }
    }

    /// The virtual address space range of the pheader
    pub fn virtual_range(&self) -> (u64, u64) {
        (self.vbegin, self.vend)
    }

    /// The offset into the string table
    pub fn pathname_offset(&self) -> Option<u32> {
        (self.pathname_offset != u32::MAX).then_some(self.pathname_offset)
    }

    /// The offset range into the referenced file
    pub fn ref_offset(&self) -> Option<u64> {
        (self.ref_offset != u64::MAX).then_some(self.ref_offset)
    }

    /// The `(index, len)` span of the itree nodes (into the itree node table)
    pub fn itree(&self) -> Option<(u32, u32)> {
        (self.itree_n_nodes != 0).then_some((self.itree_idx, self.itree_n_nodes))
    }

    /// The protections concerning this vma
    pub fn prot(&self) -> u8 {
        self.prot
    }
}

impl std::fmt::Debug for JifPheader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg_struct = f.debug_struct("JifPheader");

        dbg_struct
            .field(
                "virtual_area",
                &format!(
                    "[{:#x}, {:#x})",
                    self.virtual_range().0,
                    self.virtual_range().1
                ),
            )
            .field(
                "data_size",
                &format!("{:#x} B", self.private_pages() * PAGE_SIZE),
            );

        match self {
            JifPheader::Anonymous { itree, .. } => {
                dbg_struct.field("itree", itree);
            }
            JifPheader::Reference {
                ref_path,
                ref_offset,
                itree,
                ..
            } => {
                dbg_struct.field(
                    "ref",
                    &format!("reference {}[{:#x}..]", ref_path, ref_offset),
                );
                dbg_struct.field("itree", itree);
            }
        }

        dbg_struct
            .field(
                "prot",
                &format!(
                    "{}{}{}",
                    if self.prot() & Prot::Read as u8 != 0 {
                        "r"
                    } else {
                        "-"
                    },
                    if self.prot() & Prot::Write as u8 != 0 {
                        "w"
                    } else {
                        "-"
                    },
                    if self.prot() & Prot::Exec as u8 != 0 {
                        "x"
                    } else {
                        "-"
                    }
                ),
            )
            .finish()
    }
}

impl std::fmt::Debug for JifRawPheader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg_struct = f.debug_struct("JifPheader");

        dbg_struct.field(
            "virtual_area",
            &format!("[{:#x}, {:#x})", self.vbegin, self.vend),
        );

        if self.ref_offset != u64::MAX {
            dbg_struct.field(
                "ref",
                &format!(
                    "reference (path_offset: {:#x}), starting at {:#x}",
                    self.pathname_offset, self.ref_offset
                ),
            );
        }

        if self.itree_n_nodes > 0 {
            dbg_struct.field(
                "itree",
                &format!("[idx = {}; {}]", self.itree_idx, self.itree_n_nodes),
            );
        }

        dbg_struct
            .field(
                "prot",
                &format!(
                    "{}{}{}",
                    if self.prot & Prot::Read as u8 != 0 {
                        "r"
                    } else {
                        "-"
                    },
                    if self.prot & Prot::Write as u8 != 0 {
                        "w"
                    } else {
                        "-"
                    },
                    if self.prot & Prot::Exec as u8 != 0 {
                        "x"
                    } else {
                        "-"
                    }
                ),
            )
            .finish()
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::itree::test::*;

    pub(crate) fn gen_pheader(vaddr_range: (u64, u64), ivals: &[(u64, u64)]) -> JifPheader {
        JifPheader::Anonymous {
            vaddr_range,
            itree: ITree::build(
                ivals
                    .into_iter()
                    .map(|(start, end)| Interval {
                        start: *start,
                        end: *end,
                        data: AnonIntervalData::Owned(vec![42; (end - start) as usize]),
                    })
                    .collect(),
                vaddr_range,
            )
            .unwrap(),
            prot: Prot::Read as u8,
        }
    }

    #[test]
    fn fragment_anon_pheader() {
        let itree = gen_anon_tree();
        let mut cnt = 0;
        // ranges are mapped on and off
        // we query the midpoint in each range
        for range in VADDRS.into_iter().zip(VADDRS.into_iter().skip(1)) {
            let addr = (range.0 + range.1) / 2;
            let resolve = itree.resolve(addr);
            match cnt % 2 {
                0 => assert!(matches!(
                    &resolve.unwrap().data,
                    &AnonIntervalData::Owned(_)
                )),
                1 => assert_eq!(resolve.unwrap_err(), range),
                _ => unreachable!(),
            };
            cnt += 1
        }

        let pheader = JifPheader::Anonymous {
            vaddr_range: (VADDR_BEGIN, VADDR_END),
            itree,
            prot: Prot::Read as u8,
        };

        let prot = pheader.prot();

        let deduper = Deduper::default();
        let pheaders = pheader.fragment_vmas(&deduper, &None).unwrap();
        assert_eq!(pheaders.len(), 16);

        for (cnt, pheader) in pheaders.iter().enumerate() {
            let cnt = cnt % 2;
            assert_eq!(prot, pheader.prot());
            match pheader {
                JifPheader::Anonymous { itree, .. } if itree.n_intervals() > 0 => {
                    assert_eq!(cnt, 0)
                }
                JifPheader::Anonymous { .. } => assert_eq!(cnt, 1),
                JifPheader::Reference { .. } => {
                    assert!(false);
                }
            }
        }
    }

    #[test]
    fn fragment_ref_pheader() {
        let itree = gen_ref_tree();
        let mut cnt = 0;
        // ranges are mapped in an Owned -> Zero -> Ref cycle (Ref is implied)
        // we query the midpoint in each range
        for range in VADDRS.into_iter().zip(VADDRS.into_iter().skip(1)) {
            let addr = (range.0 + range.1) / 2;
            let resolve = itree.resolve(addr);
            match cnt % 3 {
                0 => assert!(matches!(&resolve.unwrap().data, &RefIntervalData::Owned(_))),
                1 => assert!(matches!(&resolve.unwrap().data, &RefIntervalData::Zero)),
                2 => assert_eq!(resolve.unwrap_err(), range),
                _ => unreachable!(),
            };
            cnt += 1
        }

        let pheader = JifPheader::Reference {
            vaddr_range: (VADDR_BEGIN, VADDR_END),
            itree,
            prot: Prot::Read as u8,
            ref_path: "abc".into(),
            ref_offset: 0,
        };

        let prot = pheader.prot();

        let deduper = Deduper::default();
        let pheaders = pheader.fragment_vmas(&deduper, &None).unwrap();
        assert_eq!(pheaders.len(), 16);

        for (cnt, pheader) in pheaders.iter().enumerate() {
            let cnt = cnt % 3;
            assert_eq!(prot, pheader.prot());
            match pheader {
                JifPheader::Anonymous { itree, .. } if itree.n_intervals() > 0 => {
                    assert_eq!(cnt, 0)
                }
                JifPheader::Anonymous { .. } => assert_eq!(cnt, 1),
                JifPheader::Reference { itree, .. } => {
                    assert_eq!(itree.n_intervals(), 0);
                    assert_eq!(cnt, 2);
                }
            }
        }
    }
}
