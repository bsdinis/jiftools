use std::collections::BTreeMap;

use crate::diff::{create_itree_from_diff, create_itree_from_zero_page};
use crate::error::*;
use crate::itree::ITree;
use crate::itree_node::{Interval, IntervalData, RawITreeNode};
use crate::jif::JifRaw;
use crate::utils::{page_align, PAGE_SIZE};

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};

/// VMA protection bits
#[repr(u8)]
pub enum Prot {
    Read = 1u8 << 2,
    Write = 1u8 << 1,
    Exec = 1u8 << 0,
}

/// A materialized JIF pheader
///
/// Contains all the information regarding its VMA:
///  - the address range
///  - an associated data segment and optional reference segment
///  - an interval tree
///  - protections
///
/// Can be used to visualize the VMA and manipulate it (e.g., construct an interal tree)
pub struct JifPheader {
    pub(crate) vaddr_range: (u64, u64),

    /// reference path + offset range
    pub(crate) ref_segment: Option<(String, u64)>,

    pub(crate) itree: ITree,
    pub(crate) prot: u8,
}

/// The "raw" JIF pheader
///
/// This type encodes 1:1 the information as it is serialized in the JIF format
/// It can be used to construct materialized pheaders with the help of the raw `JifRaw` type.
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
        data_map: &mut BTreeMap<(u64, u64), Vec<u8>>,
    ) -> JifResult<Self> {
        let vaddr_range = (raw.vbegin, raw.vend);

        let ref_segment = jif
            .string_at_offset(raw.pathname_offset as usize)
            .map(|s| (s.to_string(), raw.ref_offset));

        let itree = jif.get_itree(
            raw.itree_idx as usize,
            raw.itree_n_nodes as usize,
            raw.virtual_range(),
            raw.ref_offset().is_some(), /* has_reference */
            data_map,
        )?;

        if ref_segment.is_none() {
            assert_eq!(
                itree.zero_byte_size() + itree.private_data_size(),
                (vaddr_range.1 - vaddr_range.0) as usize
            );
            assert_eq!(
                itree.not_mapped_subregion_size(vaddr_range.0, vaddr_range.1),
                0
            );
        } else {
            assert_eq!(
                itree.zero_byte_size()
                    + itree.private_data_size()
                    + itree.not_mapped_subregion_size(vaddr_range.0, vaddr_range.1),
                (vaddr_range.1 - vaddr_range.0) as usize
            );
        }

        Ok(JifPheader {
            vaddr_range,
            ref_segment,
            itree,
            prot: raw.prot,
        })
    }

    /// Build an itree for a particular pheader
    pub fn build_itree(&mut self) -> JifResult<()> {
        fn build_from_diff(
            pheader: &mut JifPheader,
            overlay: Vec<u8>,
            ref_path: &str,
            ref_offset: u64,
        ) -> JifResult<ITree> {
            let mut file = {
                let mut f = BufReader::new(File::open(ref_path)?);
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
            create_itree_from_diff(&base, overlay, pheader.vaddr_range.0, &mut intervals);
            let itree = ITree::build(
                intervals,
                pheader.virtual_range(),
                true, /* has_reference */
            )?;

            assert_eq!(
                itree.zero_byte_size()
                    + itree.private_data_size()
                    + itree.not_mapped_subregion_size(pheader.vaddr_range.0, pheader.vaddr_range.1),
                (pheader.vaddr_range.1 - pheader.vaddr_range.0) as usize
            );
            Ok(itree)
        }

        fn build_from_zero(pheader: &mut JifPheader) -> JifResult<ITree> {
            let orig_itree = pheader.itree.take();
            let mut intervals = orig_itree
                .iter_intervals()
                .filter(|i| i.is_zero())
                .cloned()
                .collect();
            let data_intervals: Vec<Interval> = orig_itree
                .into_iter_intervals()
                .filter(|i| i.is_data())
                .collect();

            for data_interval in data_intervals {
                let ival_len = data_interval.len() as usize;
                if let IntervalData::Data(data) = data_interval.data {
                    assert_eq!(data.len(), ival_len);
                    create_itree_from_zero_page(data, data_interval.start, &mut intervals)
                } else {
                    panic!("we checked that this was an interval with data but there was no data");
                }
            }

            let itree = ITree::build(
                intervals,
                pheader.virtual_range(),
                false, /* has_reference */
            )?;
            assert_eq!(
                itree.zero_byte_size() + itree.private_data_size(),
                (pheader.vaddr_range.1 - pheader.vaddr_range.0) as usize
            );
            assert_eq!(
                itree.not_mapped_subregion_size(pheader.vaddr_range.0, pheader.vaddr_range.1),
                0
            );
            Ok(itree)
        }

        self.itree = if let Some((ref_path, ref_offset)) = self.ref_segment.clone() {
            if self.itree.n_data_intervals() != 1 {
                build_from_zero(self)?
            } else {
                let data_interval = self
                    .itree
                    .iter_mut_intervals()
                    .find(|i| i.is_data())
                    .expect("we checked there was a data interval");

                if data_interval.start != self.vaddr_range.0 {
                    build_from_zero(self)?
                } else {
                    let overlay = if let IntervalData::Data(ref mut data) = data_interval.data {
                        data.split_off(0)
                    } else {
                        panic!("we checked this was a data interval but there was no data");
                    };

                    build_from_diff(self, overlay, &ref_path, ref_offset)?
                }
            }
        } else {
            build_from_zero(self)?
        };

        Ok(())
    }

    /// Rename the file in this pheader if 1) it has a file and 2) it matches the name
    pub fn rename_file(&mut self, old: &str, new: &str) {
        if let Some((ref mut path, _)) = self.ref_segment {
            if path == old {
                *path = new.to_string();
            }
        }
    }

    /// Check whether this pheader maps a particular address
    pub(crate) fn mapps_addr(&self, addr: u64) -> bool {
        self.vaddr_range.0 <= addr && addr < self.vaddr_range.1
    }

    /// The virtual address space range that this pheader maps
    pub fn virtual_range(&self) -> (u64, u64) {
        self.vaddr_range
    }

    /// The pathname of the reference section
    pub fn pathname(&self) -> Option<&str> {
        self.ref_segment.as_ref().map(|(s, _)| s.as_str())
    }

    /// The offset range into the referenced file which is used to map the file data into this vma
    pub fn ref_offset(&self) -> Option<u64> {
        self.ref_segment.as_ref().map(|(_, offset)| *offset)
    }

    /// The interval tree which encodes the data source of each page
    pub fn itree(&self) -> &ITree {
        &self.itree
    }

    /// The protections concerning this vma
    pub fn prot(&self) -> u8 {
        self.prot
    }

    /// Size of the stored data (in Bytes)
    pub fn data_size(&self) -> usize {
        self.itree.private_data_size()
    }

    /// Number of zero pages encoded (by ommission) in this pheader
    pub fn zero_pages(&self) -> usize {
        self.itree.zero_byte_size() / PAGE_SIZE
    }

    /// Number of private data pages in this pheader
    pub fn private_pages(&self) -> usize {
        self.itree.private_data_size() / PAGE_SIZE
    }

    /// Number of pages coming from the reference file
    pub fn shared_pages(&self) -> usize {
        self.ref_segment
            .as_ref()
            .map(|_| {
                self.itree
                    .not_mapped_subregion_size(self.vaddr_range.0, self.vaddr_range.1)
            })
            .unwrap_or_else(|| {
                assert_eq!(
                    self.itree
                        .not_mapped_subregion_size(self.vaddr_range.0, self.vaddr_range.1),
                    0
                );
                0
            })
            / PAGE_SIZE
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
        itree_nodes: &mut Vec<RawITreeNode>,
        data_base_offset: u64,
        data_size: &mut u64,
        data_map: &mut BTreeMap<(u64, u64), Vec<u8>>,
    ) -> JifRawPheader {
        let (vbegin, vend) = jif.vaddr_range;

        let (ref_offset, pathname_offset) = if let Some((name, offset)) = jif.ref_segment {
            let pathname_offset = string_map
                .get(&name)
                .map(|path_offset| *path_offset as u32)
                .unwrap_or(u32::MAX);
            (offset, pathname_offset)
        } else {
            (u64::MAX, u32::MAX)
        };

        let (itree_idx, itree_n_nodes) = {
            let idx = itree_nodes.len() as u32;
            let len = jif.itree.nodes.len() as u32;

            itree_nodes.reserve(jif.itree.nodes.len());
            for node in jif.itree.nodes {
                let new_node =
                    RawITreeNode::from_materialized(node, data_base_offset, data_size, data_map);
                itree_nodes.push(new_node)
            }

            (idx, len)
        };

        JifRawPheader {
            vbegin,
            vend,
            ref_offset,
            itree_idx,
            itree_n_nodes,
            pathname_offset,
            prot: jif.prot,
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
                &format!("[{:#x}, {:#x})", self.vaddr_range.0, self.vaddr_range.1),
            )
            .field(
                "data_size",
                &format!("{:#x} B", self.private_pages() * PAGE_SIZE),
            );

        if let Some((path, offset)) = &self.ref_segment {
            dbg_struct.field("ref", &format!("reference {}[{:#x}..]", path, offset));
        }

        dbg_struct.field("itree", &self.itree);

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
