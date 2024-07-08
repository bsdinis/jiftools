use crate::JIF_MAGIC_HEADER;

pub type JifResult<T> = std::result::Result<T, JifError>;
#[derive(Debug)]
pub enum JifError {
    IoError(std::io::Error),
    BadMagic,
    BadHeader,
    BadAlignment,
    BadPheader {
        pheader_idx: usize,
        pheader_err: PheaderError,
    },
    BadITreeNode {
        itree_node_idx: usize,
        itree_node_err: ITreeNodeError,
    },
    DataSegmentNotFound {
        data_range: (u64, u64),
        virtual_range: (u64, u64),
    },
    UnmappedOrderingAddr(u64),
}

#[derive(Debug)]
pub enum PheaderError {
    BadAlignment(u64),
    BadVirtualRange(u64, u64),
    BadDataRange(u64, u64),
    BadRefRange {
        begin: u64,
        end: u64,
        pathname_offset: u32,
    },
    InvalidOffset {
        offset: u32,
        size: u32,
    },
    InvalidITreeIndex {
        index: u32,
        tree_len: u32,
        len: usize,
    },
}

#[derive(Debug)]
pub struct ITreeNodeError {
    pub(crate) interval_idx: usize,
    pub(crate) interval_err: IntervalError,
}

#[derive(Debug)]
pub enum IntervalError {
    BadAlignment(u64),
    BadRange(u64, u64),
    InvalidInterval(u64, u64, u64),
}

impl std::fmt::Display for JifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("jif error: ")?;
        match self {
            JifError::IoError(io) => f.write_fmt(format_args!("{}", io)),
            JifError::BadMagic => {
                f.write_str("bad magic number: ")?;
                f.debug_list().entries(JIF_MAGIC_HEADER.iter()).finish()
            }
            JifError::BadHeader => f.write_str("bad header"),
            JifError::BadAlignment => f.write_str("bad alignment"),
            JifError::BadPheader {
                pheader_idx,
                pheader_err,
            } => f.write_fmt(format_args!(
                "bad pheader (idx = {}): {}",
                pheader_idx, pheader_err
            )),
            JifError::BadITreeNode {
                itree_node_idx,
                itree_node_err,
            } => f.write_fmt(format_args!(
                "bad itree node (idx = {}): {:x?}",
                itree_node_idx, itree_node_err
            )),
            JifError::DataSegmentNotFound {
                data_range,
                virtual_range,
            } => f.write_fmt(format_args!(
                "could not find full data segment at [{:#x}; {:#x}) for pheader at [{:#x}; {:#x})",
                data_range.0, data_range.1, virtual_range.0, virtual_range.1
            )),
            JifError::UnmappedOrderingAddr(addr) => f.write_fmt(format_args!(
                "cannot insert addr {:#x} into ordering info: addr is not mapped by any pheader",
                addr
            )),
        }
    }
}

impl std::error::Error for JifError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            JifError::IoError(io) => Some(io),
            JifError::BadMagic => None,
            JifError::BadHeader => None,
            JifError::BadAlignment => None,
            JifError::BadPheader { pheader_err, .. } => Some(pheader_err),
            JifError::BadITreeNode { itree_node_err, .. } => Some(itree_node_err),
            JifError::DataSegmentNotFound { .. } => None,
            JifError::UnmappedOrderingAddr(_) => None,
        }
    }
}

impl From<std::io::Error> for JifError {
    fn from(value: std::io::Error) -> Self {
        JifError::IoError(value)
    }
}

impl std::fmt::Display for PheaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PheaderError::BadAlignment(v) => {
                f.write_fmt(format_args!("expected to be page aligned: {:x}", v))
            }
            PheaderError::BadDataRange(first, second) => f.write_fmt(format_args!(
                "invalid data range [{:#x}; {:#x}) [should be consistent and non-empty]",
                first, second
            )),
            PheaderError::BadVirtualRange(first, second) => f.write_fmt(format_args!(
                "invalid virtual range [{:#x}; {:#x}) [should be valid and non-empty]",
                first, second
            )),
            PheaderError::BadRefRange {
                begin,
                end,
                pathname_offset,
            } => f.write_fmt(format_args!(
                "invalid ref range [{:#x}; {:#x}) [should be consistent with pathname offset {:#x}]",
                begin, end, pathname_offset
            )),
            PheaderError::InvalidOffset { offset, size } => f.write_fmt(format_args!(
                "string offset ({:#x}) overflows size ({:#x})",
                offset, size
            )),
            PheaderError::InvalidITreeIndex {
                index,
                tree_len,
                len,
            } => f.write_fmt(format_args!(
                "itree node index range [{}; {}) overflows len ({})",
                index,
                index.saturating_add(*tree_len),
                len
            )),
        }
    }
}

impl std::error::Error for PheaderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl std::fmt::Display for ITreeNodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "bad interval (idx = {}): {:x?}",
            self.interval_idx, self.interval_err
        ))
    }
}

impl std::error::Error for ITreeNodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.interval_err)
    }
}

impl std::fmt::Display for IntervalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntervalError::BadAlignment(v) => {
                f.write_fmt(format_args!("expected to be page aligned: {:x}", v))
            }
            IntervalError::BadRange(first, second) => {
                f.write_fmt(format_args!("{:x} >= {:x}", first, second))
            }
            IntervalError::InvalidInterval(begin, end, offset) => f.write_fmt(format_args!(
                "invalid interval [{:x}; {:x}) -> {:x}",
                begin, end, offset
            )),
        }
    }
}

impl std::error::Error for IntervalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}
