pub type IntervalResult<T> = core::result::Result<T, IntervalError>;

/// Error parsing Intervals
#[derive(Debug)]
pub enum IntervalError {
    /// An error with IO ocurred
    IoError(std::io::Error),

    /// Value should be page aligned, but wasn't
    BadAlignment(u64),

    /// The interval range is invalid
    BadRange(u64, u64),

    /// The interval is invalid (mixed validity of fields)
    InvalidInterval(u64, u64, u64),

    /// Zero interval in anonymous segment
    ZeroIntervalInAnon,
}

impl std::fmt::Display for IntervalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("interval error: ")?;
        match self {
            IntervalError::IoError(io) => f.write_fmt(format_args!("{}", io)),
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
            IntervalError::ZeroIntervalInAnon => {
                f.write_str("anonymous segment has an explicit zero interval")
            }
        }
    }
}

impl std::error::Error for IntervalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IntervalError::IoError(io) => Some(io),
            _ => None,
        }
    }
}

impl From<std::io::Error> for IntervalError {
    fn from(value: std::io::Error) -> Self {
        IntervalError::IoError(value)
    }
}
