use crate::error::TraceReadError;
use crate::timestamped_access::TimestampedAccess;

use std::io::BufRead;

/// Read a full recorded trace
pub fn read_trace<BR: BufRead>(reader: BR) -> Result<Vec<TimestampedAccess>, TraceReadError> {
    reader
        .lines()
        .enumerate()
        .map(|(idx, line)| {
            (line?)
                .parse()
                .map_err(|error| TraceReadError::ParseError { line: idx, error })
        })
        .collect::<Result<Vec<_>, _>>()
}

// TODO: add tests
