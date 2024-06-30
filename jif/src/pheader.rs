use crate::itree::ITree;
use crate::jif::JifRaw;

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
