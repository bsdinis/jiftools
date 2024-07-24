use std::num::ParseIntError;

/// Error obtained when parsing a [`TimestampedAccess`] from a string
#[derive(Debug)]
pub enum ParseTimestampedAccessError {
    MissingDelimiter(String),
    BadTimestamp(ParseIntError),
    BadAddr(ParseIntError),
}

impl std::fmt::Display for ParseTimestampedAccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseTimestampedAccessError::MissingDelimiter(s) => f.write_fmt(format_args!(
                "missing the `:` delimiter in the log line {}",
                s
            )),
            ParseTimestampedAccessError::BadTimestamp(e) => {
                f.write_fmt(format_args!("invalid timestamp: {}", e))
            }
            ParseTimestampedAccessError::BadAddr(e) => {
                f.write_fmt(format_args!("invalid address: {}", e))
            }
        }
    }
}

impl std::error::Error for ParseTimestampedAccessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseTimestampedAccessError::MissingDelimiter(_) => None,
            ParseTimestampedAccessError::BadTimestamp(e) => Some(e),
            ParseTimestampedAccessError::BadAddr(e) => Some(e),
        }
    }
}

/// Error obtained when parsing a [`TimestampedAccess`] from a string
#[derive(Debug)]
pub enum TraceReadError {
    IoError(std::io::Error),
    ParseError {
        line: usize,
        error: ParseTimestampedAccessError,
    },
}

impl std::fmt::Display for TraceReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TraceReadError::IoError(io) => f.write_fmt(format_args!("IO error: {}", io)),
            TraceReadError::ParseError { line, error } => {
                f.write_fmt(format_args!("parse error in line {}: {}", line, error))
            }
        }
    }
}

impl std::error::Error for TraceReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TraceReadError::IoError(io) => Some(io),
            TraceReadError::ParseError { error, .. } => Some(error),
        }
    }
}

impl From<std::io::Error> for TraceReadError {
    fn from(value: std::io::Error) -> Self {
        TraceReadError::IoError(value)
    }
}
