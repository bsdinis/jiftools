pub type PheaderResult<T> = core::result::Result<T, PheaderError>;

/// Pheader error types
#[derive(Debug)]
pub enum PheaderError {
    /// An error with IO ocurred
    IoError(std::io::Error),

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

impl std::fmt::Display for PheaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("pheader error: ")?;
        match self {
            PheaderError::IoError(io) => f.write_fmt(format_args!("{}", io)),
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
        match self {
            PheaderError::IoError(io) => Some(io),
            _ => None,
        }
    }
}

impl From<std::io::Error> for PheaderError {
    fn from(value: std::io::Error) -> Self {
        PheaderError::IoError(value)
    }
}
