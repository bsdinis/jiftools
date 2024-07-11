//! JIF error types
//!
//! Encodes the error types possible when parsing/writing and manipulating JIF files

use crate::JIF_MAGIC_HEADER;

/// JIF result type
pub type JifResult<T> = std::result::Result<T, JifError>;

/// JIF error type
#[derive(Debug)]
pub enum JifError {
    /// An error with IO ocurred
    IoError(std::io::Error),

    /// Bad JIF magic header
    BadMagic,

    /// Ill-formed JIF header
    BadHeader,

    /// A particular section was poorly aligned
    BadAlignment,

    /// Error with a particular pheader
    BadPheader {
        pheader_idx: usize,
        pheader_err: PheaderError,
    },

    /// Error with a particular itree node
    BadITreeNode {
        itree_node_idx: usize,
        itree_node_err: ITreeNodeError,
    },

    /// Error with an ord chunk
    BadOrdChunk {
        ord_chunk_idx: usize,
        ord_chunk_err: OrdChunkError,
    },

    /// Could not find a particular data segment mentioned by a particular virtual address range
    DataSegmentNotFound {
        /// Requested data range
        data_range: (u64, u64),

        /// Corresponding virtual range
        virtual_range: (u64, u64),

        /// Data length found
        found_len: usize,
    },

    /// Could not find an itree
    ITreeNotFound {
        /// itree index
        index: usize,

        /// itree len
        len: usize,

        /// number of nodes in the JIF
        n_nodes: usize,
    },

    /// The provided interval tree is badly formed
    InvalidITree {
        virtual_range: (u64, u64),
        error: ITreeError,
    },

    /// A requested ordering address is not mapped by this JIF
    UnmappedOrderingAddr(u64),
}

/// Pheader error types
#[derive(Debug)]
pub enum PheaderError {
    /// The integer should have been page aligned, but wasn't
    BadAlignment(u64),

    /// Invalid virtual range
    BadVirtualRange(u64, u64),

    /// Invalid reference range
    BadRefRange { offset: u64, pathname_offset: u32 },

    /// Invalid string offset
    InvalidOffset { offset: u32, size: u32 },

    /// Invalid itree index
    InvalidITreeIndex {
        index: u32,
        tree_len: u32,
        len: usize,
    },
}

/// Error parsing `ITreeNode`s
#[derive(Debug)]
pub struct ITreeNodeError {
    pub(crate) interval_idx: usize,
    pub(crate) interval_err: IntervalError,
}

/// Error parsing Intervals
#[derive(Debug)]
pub enum IntervalError {
    /// Value should be page aligned, but wasn't
    BadAlignment(u64),

    /// The interval range is invalid
    BadRange(u64, u64),

    /// The interval is invalid (mixed validity of fields)
    InvalidInterval(u64, u64, u64),
}

/// Pheader error types
#[derive(Debug)]
pub enum OrdChunkError {
    /// The integer should have been page aligned, but wasn't
    BadAlignment(u64),
}

/// ITree error types
#[derive(Debug)]
pub enum ITreeError {
    /// Non reference pheaders need to be fully mapped by their zero and private sections
    NonReferenceNotCovered {
        expected_coverage: usize,
        covered_by_zero: usize,
        covered_by_private: usize,
    },

    /// Non reference pheaders cannot have non mapped regions
    NonReferenceHoled { non_mapped: usize },

    /// Reference pheader does not add up
    ReferenceNotCovered {
        expected_coverage: usize,
        covered_by_zero: usize,
        covered_by_private: usize,
        non_mapped: usize,
    },

    /// Intervals cannot intersect
    IntersectingInterval {
        interval_1: (u64, u64),
        interval_2: (u64, u64),
    },

    /// Interval out of the virtual address range
    IntervalOutOfRange { interval: (u64, u64) },
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
            JifError::BadOrdChunk {
                ord_chunk_idx,
                ord_chunk_err,
            } => f.write_fmt(format_args!(
                "bad ord chunk (idx = {}): {}",
                ord_chunk_idx, ord_chunk_err
            )),
            JifError::BadITreeNode {
                itree_node_idx,
                itree_node_err,
            } => f.write_fmt(format_args!(
                "bad itree node (idx = {}): {:#x?}",
                itree_node_idx, itree_node_err
            )),
            JifError::InvalidITree { virtual_range, error } => {
                f.write_fmt(format_args!("bad itree [{:#x}; {:#x}): {}", virtual_range.0, virtual_range.1, error))
            }
            JifError::DataSegmentNotFound {
                data_range,
                virtual_range,
                found_len
            } => f.write_fmt(format_args!(
                "could not find full data segment at [{:#x}; {:#x}) for pheader at [{:#x}; {:#x}), found {:#x} of the requested {:#x} B",
                data_range.0, data_range.1, virtual_range.0, virtual_range.1, found_len, data_range.1 - data_range.0
            )),
            JifError::ITreeNotFound {
                index,
                len,
                n_nodes,
            } => f.write_fmt(format_args!(
                "could not find full interval tree at [{}; {}) (there are only {} itree nodes)",
                index, len, n_nodes
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
            JifError::BadOrdChunk { ord_chunk_err, .. } => Some(ord_chunk_err),
            JifError::InvalidITree { error, .. } => Some(error),
            JifError::DataSegmentNotFound { .. } => None,
            JifError::ITreeNotFound { .. } => None,
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
            PheaderError::BadVirtualRange(first, second) => f.write_fmt(format_args!(
                "invalid virtual range [{:#x}; {:#x}) [should be valid]",
                first, second
            )),
            PheaderError::BadRefRange {
                offset,
                pathname_offset,
            } => f.write_fmt(format_args!(
                "invalid ref offset {:#x} [should be consistent with pathname offset {:#x}]",
                offset, pathname_offset
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

impl std::fmt::Display for OrdChunkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrdChunkError::BadAlignment(v) => f.write_fmt(format_args!(
                "expected virtual address to be page aligned: {:x}",
                v
            )),
        }
    }
}

impl std::error::Error for OrdChunkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
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

impl std::fmt::Display for ITreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ITreeError::NonReferenceHoled { non_mapped } => f.write_fmt(format_args!(
                "non reference interval has {} bytes that are not mapped",
                non_mapped
            )),
            ITreeError::NonReferenceNotCovered {
                expected_coverage,
                covered_by_zero,
                covered_by_private,
            } => f.write_fmt(format_args!("non reference interval needs {:#x} B to be covered and {:#x} B are covered by zero pages and {:#x} B by private data - {:#x} B missing",
                    expected_coverage, covered_by_zero, covered_by_private, expected_coverage - covered_by_private - covered_by_zero
                    )),
            ITreeError::ReferenceNotCovered {
                expected_coverage,
                covered_by_zero,
                covered_by_private,
                non_mapped
            } => f.write_fmt(format_args!("reference interval needs {:#x} B to be covered and {:#x} B are covered by zero pages, {:#x} B by private data and {:#x} B not mapped - {:#x} B missing",
                    expected_coverage, covered_by_zero, covered_by_private, non_mapped, expected_coverage - covered_by_private - covered_by_zero - non_mapped
                    )),
            ITreeError::IntersectingInterval {
                interval_1,
                interval_2,
            } => f.write_fmt(format_args!(
                "intervals are intersecting: [{:#x}; {:#x}) and [{:#x}; {:#x})",
                interval_1.0, interval_1.1, interval_2.0, interval_2.1
            )),
            ITreeError::IntervalOutOfRange { interval } => f.write_fmt(format_args!("interval [{:#x}; {:#x}) is out of range", interval.0, interval.1))
        }
    }
}

impl std::error::Error for ITreeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}
