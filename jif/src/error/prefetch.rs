pub type WindowingStrategyResult<T> = core::result::Result<T, WindowingStrategyError>;
pub type PrefetchWindowResult<T> = core::result::Result<T, PrefetchWindowError>;

/// Windowing strategy error
#[derive(Debug)]
pub enum WindowingStrategyError {
    /// An error with IO ocurred
    IoError(std::io::Error),

    /// There are more write pages than total
    InvalidId(u32),
}

impl std::fmt::Display for WindowingStrategyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("windowing strategy error: ")?;
        match self {
            WindowingStrategyError::IoError(io) => f.write_fmt(format_args!("{}", io)),
            WindowingStrategyError::InvalidId(id) => {
                f.write_fmt(format_args!("invalid id for a windowing strategy {id}",))
            }
        }
    }
}

impl std::error::Error for WindowingStrategyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WindowingStrategyError::IoError(io) => Some(io),
            _ => None,
        }
    }
}

impl From<std::io::Error> for WindowingStrategyError {
    fn from(value: std::io::Error) -> Self {
        WindowingStrategyError::IoError(value)
    }
}

/// Prefetch window error type
#[derive(Debug)]
pub enum PrefetchWindowError {
    /// An error with IO ocurred
    IoError(std::io::Error),

    /// There are more write pages than total
    InvalidFraction { write: u64, total: u64 },
}

impl std::fmt::Display for PrefetchWindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("prefetch window error: ")?;
        match self {
            PrefetchWindowError::IoError(io) => f.write_fmt(format_args!("{}", io)),
            PrefetchWindowError::InvalidFraction { write, total } => f.write_fmt(format_args!(
                "cannot have more write pages than total in the window: {:x} > {:x}",
                write, total
            )),
        }
    }
}

impl std::error::Error for PrefetchWindowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PrefetchWindowError::IoError(io) => Some(io),
            _ => None,
        }
    }
}

impl From<std::io::Error> for PrefetchWindowError {
    fn from(value: std::io::Error) -> Self {
        PrefetchWindowError::IoError(value)
    }
}
