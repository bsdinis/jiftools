use crate::error::TraceReadError;
use crate::timestamped_access::TimestampedAccess;

use std::collections::HashMap;
use std::io::BufRead;

/// Read a full recorded trace
pub fn read_trace<BR: BufRead>(reader: BR) -> Result<Vec<TimestampedAccess>, TraceReadError> {
    reader
        .lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            match line {
                Ok(ref l) if l.trim_start().starts_with("#") => None, // Skip lines starting with #
                Ok(l) => Some(
                    l.parse()
                        .map_err(|error| TraceReadError::ParseError { line: idx, error }),
                ),
                Err(e) => Some(Err(TraceReadError::IoError(e))),
            }
        })
        .collect::<Result<Vec<_>, _>>()
}

/// Dedup a trace
pub fn dedup_trace(log: Vec<TimestampedAccess>) -> Vec<TimestampedAccess> {
    // deduping:
    // construct an addr -> access hashmap map, where we keep only the first access
    let mut map = HashMap::with_capacity(log.len());
    for tsa in log.into_iter().map(|mut tsa| {
        tsa.truncate_addr();
        tsa
    }) {
        // keep the most recent entry
        map.entry(tsa.raw_addr())
            .and_modify(|existing| {
                if tsa < *existing {
                    *existing = tsa
                }
            })
            .or_insert(tsa);
    }

    map.into_values().collect::<Vec<_>>()
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;

    use crate::ParseTimestampedAccessError;

    use super::*;

    #[test]
    fn parse_ok() {
        assert_eq!(read_trace("".as_bytes()).unwrap(), vec![]);
        assert_eq!(
            read_trace("01234: 0xdead".as_bytes()).unwrap(),
            vec![TimestampedAccess {
                usecs: 1234,
                addr: 0xdead
            }]
        );
        assert_eq!(
            read_trace("01234: 0xdead".as_bytes()).unwrap(),
            vec![TimestampedAccess {
                usecs: 1234,
                addr: 0xdead
            }]
        );
        assert_eq!(
            read_trace("01234: 0xdead\n4: 1234".as_bytes()).unwrap(),
            vec![
                TimestampedAccess {
                    usecs: 1234,
                    addr: 0xdead
                },
                TimestampedAccess {
                    usecs: 4,
                    addr: 1234
                },
            ]
        );
        assert_eq!(
            read_trace("01234: 0xdead\n4: 1234\n1234: 0xdead".as_bytes()).unwrap(),
            vec![
                TimestampedAccess {
                    usecs: 1234,
                    addr: 0xdead
                },
                TimestampedAccess {
                    usecs: 4,
                    addr: 1234
                },
                TimestampedAccess {
                    usecs: 1234,
                    addr: 0xdead
                },
            ]
        );
    }

    #[test]
    fn parse_err() {
        assert!(matches!(
            read_trace("s".as_bytes()),
            Err(TraceReadError::ParseError {
                line: 0,
                error: ParseTimestampedAccessError::MissingDelimiter(_)
            })
        ));
        assert!(matches!(
            read_trace("a: 0xdead".as_bytes()),
            Err(TraceReadError::ParseError {
                line: 0,
                error: ParseTimestampedAccessError::BadTimestamp(_)
            })
        ));
        assert!(matches!(
            read_trace("01234: 0xdead\n4: 1234\n4: asdf".as_bytes()),
            Err(TraceReadError::ParseError {
                line: 2,
                error: ParseTimestampedAccessError::BadAddr(_)
            })
        ));
    }

    #[test]
    fn dedup_0() {
        let original = vec![
            TimestampedAccess {
                usecs: 1,
                addr: 0x1000,
            },
            TimestampedAccess {
                usecs: 2,
                addr: 0x3000,
            },
            TimestampedAccess {
                usecs: 3,
                addr: 0x2000,
            },
        ];

        assert_eq!(
            dedup_trace(original).into_iter().collect::<HashSet<_>>(),
            [
                TimestampedAccess {
                    usecs: 1,
                    addr: 0x1000
                },
                TimestampedAccess {
                    usecs: 3,
                    addr: 0x2000
                },
                TimestampedAccess {
                    usecs: 2,
                    addr: 0x3000
                },
            ]
            .into_iter()
            .collect::<HashSet<_>>()
        )
    }

    #[test]
    fn dedup_1() {
        let original = vec![
            TimestampedAccess {
                usecs: 1,
                addr: 0x1000,
            },
            TimestampedAccess {
                usecs: 2,
                addr: 0x3000,
            },
            TimestampedAccess {
                usecs: 4,
                addr: 0x2000,
            },
            TimestampedAccess {
                usecs: 3,
                addr: 0x2000,
            },
            TimestampedAccess {
                usecs: 2,
                addr: 0x1000,
            },
        ];

        assert_eq!(
            dedup_trace(original).into_iter().collect::<HashSet<_>>(),
            [
                TimestampedAccess {
                    usecs: 1,
                    addr: 0x1000
                },
                TimestampedAccess {
                    usecs: 3,
                    addr: 0x2000
                },
                TimestampedAccess {
                    usecs: 2,
                    addr: 0x3000
                },
            ]
            .into_iter()
            .collect::<HashSet<_>>()
        )
    }
}
