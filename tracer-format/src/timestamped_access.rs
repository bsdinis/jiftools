use crate::error::ParseTimestampedAccessError;

use std::str::FromStr;

/// Representation of an entry in the log of recorded adresses in a Junction tracer output
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct TimestampedAccess {
    pub usecs: usize,
    pub(crate) addr: usize,
}

impl TimestampedAccess {
    const ORD_WRITE_FLAG: usize = (1 << 60);
    const ORD_FLAG_MASK: usize = Self::ORD_WRITE_FLAG - 1;

    /// make addr page aligned (we only care about pages when prefetching)
    pub fn truncate_addr(&mut self) {
        self.addr &= !0xfff;
    }

    /// return the raw address (even if it has certain metadata bits turned on)
    pub fn raw_addr(&self) -> usize {
        self.addr
    }

    /// return the actual memory address
    pub fn masked_addr(&self) -> usize {
        self.addr & Self::ORD_FLAG_MASK
    }
}

impl PartialOrd for TimestampedAccess {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self.usecs.cmp(&other.usecs), self.addr == other.addr) {
            (std::cmp::Ordering::Equal, true) => Some(std::cmp::Ordering::Equal),
            (std::cmp::Ordering::Equal, false) => None,
            (a, _) => Some(a),
        }
    }
}

impl FromStr for TimestampedAccess {
    type Err = ParseTimestampedAccessError;
    /// parse a line in the log of accesses
    ///
    /// `<usecs>: <address>`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (usec_str, addr_str) = s
            .split_once(':')
            .ok_or_else(|| ParseTimestampedAccessError::MissingDelimiter(s.to_string()))?;

        let usecs = usec_str
            .trim()
            .parse::<usize>()
            .map_err(ParseTimestampedAccessError::BadTimestamp)?;
        let addr = if let Some(hex_str) = addr_str.trim().strip_prefix("0x") {
            usize::from_str_radix(hex_str, 0x10)
        } else {
            addr_str.trim().parse::<usize>()
        }
        .map_err(ParseTimestampedAccessError::BadAddr)?;

        Ok(TimestampedAccess { usecs, addr })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_parse() {
        assert_eq!(
            "1234: 5678".parse::<TimestampedAccess>().unwrap(),
            TimestampedAccess {
                usecs: 1234,
                addr: 5678
            }
        );
        assert_eq!(
            "1234: 0x1234".parse::<TimestampedAccess>().unwrap(),
            TimestampedAccess {
                usecs: 1234,
                addr: 0x1234
            }
        );
    }

    #[test]
    fn err_parse() {
        assert!(matches!(
            "0xdead: 0x1234".parse::<TimestampedAccess>(),
            Err(ParseTimestampedAccessError::BadTimestamp(_)),
        ));
        assert!(matches!(
            "notanumber: 0x1234".parse::<TimestampedAccess>(),
            Err(ParseTimestampedAccessError::BadTimestamp(_)),
        ));
        assert!(matches!(
            "notanumber: alsonotanumber".parse::<TimestampedAccess>(),
            Err(ParseTimestampedAccessError::BadTimestamp(_)),
        ));
        assert!(matches!(
            "1234: notanumber".parse::<TimestampedAccess>(),
            Err(ParseTimestampedAccessError::BadAddr(_)),
        ));
        assert!(matches!(
            "1234  0x1234".parse::<TimestampedAccess>(),
            Err(ParseTimestampedAccessError::MissingDelimiter(_)),
        ));
    }

    #[test]
    fn cmp() {
        assert!(
            TimestampedAccess {
                usecs: 1234,
                addr: 0xffff
            } < TimestampedAccess {
                usecs: 5678,
                addr: 0x0000
            }
        );
        assert!(
            TimestampedAccess {
                usecs: 5678,
                addr: 0xffff
            } > TimestampedAccess {
                usecs: 1234,
                addr: 0x0000
            }
        );
        assert!(
            TimestampedAccess {
                usecs: 1234,
                addr: 0x0000
            } == TimestampedAccess {
                usecs: 1234,
                addr: 0x0000
            }
        );
    }
}
