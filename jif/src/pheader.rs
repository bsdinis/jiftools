use std::collections::BTreeMap;

use crate::itree::{ITree, ITreeNode};
use crate::jif::JifRaw;
use crate::{create_itree_from_diff, create_itree_from_zero_page, error::*};

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};

#[repr(u8)]
pub enum Prot {
    Read = 1u8 << 3,
    Write = 1u8 << 2,
    Exec = 1u8 << 1,
}

pub struct JifPheader {
    pub(crate) vaddr_range: (u64, u64),
    pub(crate) data_segment: Vec<u8>,

    /// reference path + offset range
    pub(crate) ref_range: Option<(String, u64, u64)>,

    pub(crate) itree: Option<ITree>,
    pub(crate) prot: u8,
}

pub struct JifRawPheader {
    pub(crate) vbegin: u64,
    pub(crate) vend: u64,

    pub(crate) data_begin: u64,
    pub(crate) data_end: u64,

    pub(crate) ref_begin: u64,
    pub(crate) ref_end: u64,

    pub(crate) itree_idx: u32,
    pub(crate) itree_n_nodes: u32,

    pub(crate) pathname_offset: u32,

    pub(crate) prot: u8,
}

impl JifPheader {
    pub(crate) fn from_raw(
        jif: &JifRaw,
        raw: &JifRawPheader,
        data_segments: &mut BTreeMap<u64, Vec<u8>>,
    ) -> JifResult<Self> {
        fn extract_data_segment(
            data_segments: &mut BTreeMap<u64, Vec<u8>>,
            begin: u64,
            end: u64,
        ) -> Option<Vec<u8>> {
            let key = data_segments
                .iter()
                .find(|(addr, data)| {
                    **addr <= begin && end as usize <= **addr as usize + data.len()
                })
                .map(|(addr, _)| addr)
                .copied();

            if let Some(addr) = key {
                let mut data = data_segments.remove(&addr).expect("we just found this key");
                let offset = begin - addr;

                // cut off region before `begin`
                let mut segment = if offset == 0 {
                    data
                } else {
                    let segment = data.split_off(offset as usize);
                    data_segments.insert(addr, data);
                    segment
                };

                // cut off region after end
                let len = end - begin;
                if segment.len() > len as usize {
                    let leftover = segment.split_off(len as usize);
                    data_segments.insert(end, leftover);
                }

                Some(segment)
            } else {
                None
            }
        }

        let vaddr_range = (raw.vbegin, raw.vend);
        let data_segment = extract_data_segment(data_segments, raw.data_begin, raw.data_end)
            .ok_or(JifError::DataSegmentNotFound {
                begin: raw.data_begin,
                end: raw.data_end,
            })?;

        let ref_range = jif
            .string_at_offset(raw.pathname_offset as usize)
            .map(|s| (s.to_string(), raw.ref_begin, raw.ref_end));

        let itree = {
            let mut it = jif.get_itree(raw.itree_idx as usize, raw.itree_n_nodes as usize);
            if let Some(ref mut itree) = it {
                itree.shift_offsets(-(raw.data_begin as i64));
            }
            it
        };

        Ok(JifPheader {
            vaddr_range,
            data_segment,
            ref_range,
            itree,
            prot: raw.prot,
        })
    }

    pub fn build_itree(&mut self) -> JifResult<()> {
        if let Some((ref_path, ref_begin, ref_end)) = &self.ref_range {
            let len = ref_end - ref_begin;

            let mut file = {
                let mut f = BufReader::new(File::open(ref_path)?);
                f.seek(SeekFrom::Start(*ref_begin))?;
                f.take(len)
            };

            let base = {
                let mut buf = Vec::with_capacity(len as usize);
                file.read_to_end(&mut buf)?;
                buf
            };

            self.itree = Some(create_itree_from_diff(
                &base,
                &mut self.data_segment,
                self.vaddr_range.0,
            )?);
        } else {
            self.itree = Some(create_itree_from_zero_page(
                &mut self.data_segment,
                self.vaddr_range.0,
            )?);
        }

        Ok(())
    }

    pub fn rename_file(&mut self, old: &str, new: &str) {
        if let Some((ref mut path, _, _)) = self.ref_range {
            if path == old {
                *path = new.to_string();
            }
        }
    }
}

impl JifRawPheader {
    pub const fn serialized_size() -> usize {
        6 * std::mem::size_of::<u64>() + 3 * std::mem::size_of::<u32>() + std::mem::size_of::<u8>()
    }

    pub(crate) fn from_materialized(
        mut jif: JifPheader,
        string_map: &BTreeMap<String, usize>,
        itree_nodes: &mut Vec<ITreeNode>,
        data_cursor: &mut u64,
        data: &mut Vec<u8>,
    ) -> JifRawPheader {
        let (vbegin, vend) = jif.vaddr_range;

        let data_begin = *data_cursor;
        *data_cursor += jif.data_segment.len() as u64;
        let data_end = *data_cursor;
        data.append(&mut jif.data_segment);

        let (ref_begin, ref_end, pathname_offset) = if let Some((name, begin, end)) = jif.ref_range
        {
            let offset = string_map
                .get(&name)
                .map(|offset| *offset as u32)
                .unwrap_or(u32::MAX);
            (begin, end, offset)
        } else {
            (u64::MAX, u64::MAX, u32::MAX)
        };

        let (itree_idx, itree_n_nodes) = if let Some(mut itree) = jif.itree {
            let idx = itree_nodes.len() as u32;
            let len = itree.nodes.len() as u32;

            itree.shift_offsets(data_begin as i64);
            itree_nodes.append(&mut itree.nodes);
            (idx, len)
        } else {
            (u32::MAX, 0)
        };

        JifRawPheader {
            vbegin,
            vend,
            data_begin,
            data_end,
            ref_begin,
            ref_end,
            itree_idx,
            itree_n_nodes,
            pathname_offset,
            prot: jif.prot,
        }
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
            .field("data_size", &format!("{:#x} B", self.data_segment.len()));

        if let Some((path, start, end)) = &self.ref_range {
            dbg_struct.field(
                "ref",
                &format!("[{:#x}, {:#x}) (path: {})", start, end, path),
            );
        }

        if let Some(itree) = &self.itree {
            dbg_struct.field("itree", &itree);
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

impl std::fmt::Debug for JifRawPheader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg_struct = f.debug_struct("JifPheader");

        dbg_struct
            .field(
                "virtual_area",
                &format!("[{:#x}, {:#x})", self.vbegin, self.vend),
            )
            .field(
                "data",
                &format!("[{:#x}, {:#x})", self.data_begin, self.data_end),
            );

        if self.ref_begin != u64::MAX {
            dbg_struct.field(
                "ref",
                &format!(
                    "[{:#x}, {:#x}) (path_offset: {:#x})",
                    self.ref_begin, self.ref_end, self.pathname_offset
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
