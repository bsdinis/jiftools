use crate::error::*;
use crate::itree::ITree;
use crate::jif::JifRaw;
use crate::utils::{is_page_aligned, read_u32, read_u64, read_u8};

use std::io::Read;

#[repr(u8)]
pub enum Prot {
    Read = 1u8 << 3,
    Write = 1u8 << 2,
    Exec = 1u8 << 1,
}

pub struct JifPheader {
    vaddr_range: (u64, u64),
    data_range: (u64, u64),

    /// reference path + offset range
    ref_range: Option<(String, u64, u64)>,

    itree: Option<ITree>,
    prot: u8,
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
    pub(crate) fn from_raw(jif: &JifRaw, raw: &JifRawPheader) -> Self {
        let vaddr_range = (raw.vbegin, raw.vend);
        let data_range = (raw.data_begin, raw.data_end);

        let ref_range = jif
            .string_at_offset(raw.pathname_offset as usize)
            .map(|s| (s.to_string(), raw.ref_begin, raw.ref_end));

        let itree = jif.get_itree(raw.itree_idx as usize, raw.itree_n_nodes as usize);

        JifPheader {
            vaddr_range,
            data_range,
            ref_range,
            itree,
            prot: raw.prot,
        }
    }
}

impl JifRawPheader {
    pub const fn serialized_size() -> usize {
        6 * std::mem::size_of::<u64>() + 3 * std::mem::size_of::<u32>() + std::mem::size_of::<u8>()
    }

    pub fn from_reader<R: Read>(r: &mut R, pheader_idx: usize) -> JifResult<Self> {
        fn read_page_aligned_u64<R: Read>(
            r: &mut R,
            buffer: &mut [u8; 8],
            special_value: bool,
            pheader_idx: usize,
        ) -> JifResult<u64> {
            let v = read_u64(r, buffer)?;

            // MAX is a special value
            if special_value && v == u64::MAX {
                return Ok(v);
            }

            if !is_page_aligned(v) {
                Err(JifError::BadPheader {
                    pheader_idx,
                    pheader_err: PheaderError::BadAlignment(v),
                })
            } else {
                Ok(v)
            }
        }
        fn read_page_aligned_u64_pair<R: Read>(
            r: &mut R,
            buffer: &mut [u8; 8],
            special_value: bool,
            pheader_idx: usize,
        ) -> JifResult<(u64, u64)> {
            let begin = read_page_aligned_u64(r, buffer, special_value, pheader_idx)?;
            let end = read_page_aligned_u64(r, buffer, special_value, pheader_idx)?;

            if special_value {
                if begin == u64::MAX && end == u64::MAX {
                    return Ok((begin, end));
                } else if begin == u64::MAX {
                    return Err(JifError::BadPheader {
                        pheader_idx,
                        pheader_err: PheaderError::BadAlignment(begin),
                    });
                } else if end == u64::MAX {
                    return Err(JifError::BadPheader {
                        pheader_idx,
                        pheader_err: PheaderError::BadAlignment(end),
                    });
                }
            }

            if begin >= end {
                Err(JifError::BadPheader {
                    pheader_idx,
                    pheader_err: PheaderError::BadRange(begin, end),
                })
            } else {
                Ok((begin, end))
            }
        }

        let mut buffer_8 = [0u8; 8];
        let (vbegin, vend) =
            read_page_aligned_u64_pair(r, &mut buffer_8, false /* special */, pheader_idx)?;
        let (data_begin, data_end) =
            read_page_aligned_u64_pair(r, &mut buffer_8, false /* special */, pheader_idx)?;
        let (ref_begin, ref_end) =
            read_page_aligned_u64_pair(r, &mut buffer_8, true /* special */, pheader_idx)?;

        let mut buffer_4 = [0u8; 4];
        let itree_idx = read_u32(r, &mut buffer_4)?;
        let itree_n_nodes = read_u32(r, &mut buffer_4)?;

        let pathname_offset = read_u32(r, &mut buffer_4)?;

        if ref_begin == u64::MAX && pathname_offset != u32::MAX
            || ref_begin != u64::MAX && pathname_offset == u32::MAX
        {
            return Err(JifError::BadPheader {
                pheader_idx,
                pheader_err: PheaderError::BadRefRange {
                    begin: ref_begin,
                    end: ref_end,
                    pathname_offset,
                },
            });
        }

        let mut buffer_1 = [0u8; 1];
        let prot = read_u8(r, &mut buffer_1)?;

        Ok(JifRawPheader {
            vbegin,
            vend,
            data_begin,
            data_end,
            ref_begin,
            ref_end,
            itree_idx,
            itree_n_nodes,
            pathname_offset,
            prot,
        })
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
                "data",
                &format!("[{:#x}, {:#x})", self.data_range.0, self.data_range.1),
            );

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
                    "[{:#x}, {:#x}) (path_offset: {})",
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
