use crate::error::*;
use crate::interval::RawInterval;

use std::io::Write;

impl RawInterval {
    /// Write an interval
    pub fn to_writer<W: Write>(self, w: &mut W) -> JifResult<usize> {
        w.write_all(&self.start.to_le_bytes())?;
        w.write_all(&self.end.to_le_bytes())?;
        w.write_all(&self.offset.to_le_bytes())?;
        Ok(RawInterval::serialized_size())
    }
}
