pub type OrdChunkResult<T> = core::result::Result<T, OrdChunkError>;

/// Ord error type
#[derive(Debug)]
pub enum OrdChunkError {
    /// An error with IO ocurred
    IoError(std::io::Error),

    /// The integer should have been page aligned, but wasn't
    BadAlignment(u64),
}

impl std::fmt::Display for OrdChunkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ord chunk error: ")?;
        match self {
            OrdChunkError::IoError(io) => f.write_fmt(format_args!("{}", io)),
            OrdChunkError::BadAlignment(v) => f.write_fmt(format_args!(
                "expected virtual address to be page aligned: {:x}",
                v
            )),
        }
    }
}

impl std::error::Error for OrdChunkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            OrdChunkError::IoError(io) => Some(io),
            _ => None,
        }
    }
}

impl From<std::io::Error> for OrdChunkError {
    fn from(value: std::io::Error) -> Self {
        OrdChunkError::IoError(value)
    }
}
