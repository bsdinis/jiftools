//! JIF error types
//!
//! Encodes the error types possible when parsing/writing and manipulating JIF files

use crate::error::itree::ITreeError;
use crate::error::itree_node::ITreeNodeError;
use crate::error::ord::OrdChunkError;
use crate::error::pheader::PheaderError;
use crate::jif::JIF_MAGIC_HEADER;

/// JIF result type
pub type JifResult<T> = core::result::Result<T, JifError>;

/// JIF error type
#[derive(Debug)]
pub enum JifError {
    /// An error with IO ocurred
    IoError(std::io::Error),

    /// Bad JIF magic header
    BadMagic,

    /// Ill-formed JIF header
    BadHeader,

    // Version mismatch.
    BadVersion {
        expected: u32,
        found: u32,
    },

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
            JifError::BadVersion { expected, found } => {
                f.write_str("bad version, expected v")?;
                expected.fmt(f)?;
                f.write_str("found v")?;
                found.fmt(f)
            }
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
            JifError::BadVersion { .. } => None,
            JifError::BadPheader { pheader_err, .. } => Some(pheader_err),
            JifError::BadITreeNode { itree_node_err, .. } => Some(itree_node_err),
            JifError::BadOrdChunk { ord_chunk_err, .. } => Some(ord_chunk_err),
            JifError::InvalidITree { error, .. } => Some(error),
            JifError::DataSegmentNotFound { .. } => None,
            JifError::ITreeNotFound { .. } => None,
        }
    }
}

impl From<std::io::Error> for JifError {
    fn from(value: std::io::Error) -> Self {
        JifError::IoError(value)
    }
}
