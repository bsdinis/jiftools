pub type ITreeResult<T> = core::result::Result<T, ITreeError>;

/// ITree error types
#[derive(Debug)]
pub enum ITreeError {
    /// An error with IO ocurred
    IoError(std::io::Error),

    /// Non reference pheaders need to be fully mapped by their zero and private sections
    RangeNotCovered {
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

impl std::fmt::Display for ITreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("itree error: ")?;
        match self {
            ITreeError::IoError(io) => f.write_fmt(format_args!("{}", io)),
            ITreeError::RangeNotCovered {
                expected_coverage,
                covered_by_zero,
                covered_by_private,
                non_mapped,
            } => f.write_fmt(format_args!("interval needs {:#x} B to be covered and {:#x} B are covered by zero pages, {:#x} B by private data and {:#x} B not mapped - {:#x} B missing",
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
        match self {
            ITreeError::IoError(io) => Some(io),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ITreeError {
    fn from(value: std::io::Error) -> Self {
        ITreeError::IoError(value)
    }
}
