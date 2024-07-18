use crate::error::*;
use crate::itree::interval::RawInterval;
use crate::utils::{is_page_aligned, read_u64};

use std::io::Read;

impl RawInterval {
    /// Read and parse a RawInterval
    pub fn from_reader<R: Read>(r: &mut R) -> IntervalResult<Self> {
        fn read_page_aligned_u64<R: Read>(r: &mut R, buffer: &mut [u8; 8]) -> IntervalResult<u64> {
            let v = read_u64(r, buffer)?;

            // MAX is a special value
            if v == u64::MAX {
                return Ok(v);
            }

            if !is_page_aligned(v) {
                Err(IntervalError::BadAlignment(v))
            } else {
                Ok(v)
            }
        }

        let mut buffer = [0u8; 8];

        let start = read_page_aligned_u64(r, &mut buffer)?;
        let end = read_page_aligned_u64(r, &mut buffer)?;
        let offset = read_page_aligned_u64(r, &mut buffer)?;

        if start == u64::MAX || end == u64::MAX {
            if start == end && offset == u64::MAX {
                // this is a default Interval
                return Ok(RawInterval::default());
            } else {
                return Err(IntervalError::InvalidInterval(start, end, offset));
            }
        }

        if start > end {
            return Err(IntervalError::BadRange(start, end));
        }

        Ok(RawInterval::new(start, end, offset))
    }
}
