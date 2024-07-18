use crate::error::interval::IntervalError;

pub type ITreeNodeResult<T> = core::result::Result<T, ITreeNodeError>;

/// Error parsing `ITreeNode`s
#[derive(Debug)]
pub enum ITreeNodeError {
    /// An error with IO ocurred
    IoError(std::io::Error),

    /// An error with one of the inner intervals ocurred
    Interval {
        interval_idx: usize,
        interval_err: IntervalError,
    },
}

impl std::fmt::Display for ITreeNodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("itree node error: ")?;
        match self {
            ITreeNodeError::IoError(io) => f.write_fmt(format_args!("{}", io)),
            ITreeNodeError::Interval {
                interval_idx,
                interval_err,
            } => f.write_fmt(format_args!(
                "bad interval (idx = {}): {:x?}",
                interval_idx, interval_err
            )),
        }
    }
}

impl std::error::Error for ITreeNodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ITreeNodeError::IoError(io) => Some(io),
            ITreeNodeError::Interval { interval_err, .. } => Some(interval_err),
        }
    }
}

impl From<std::io::Error> for ITreeNodeError {
    fn from(value: std::io::Error) -> Self {
        ITreeNodeError::IoError(value)
    }
}
