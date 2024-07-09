use crate::error::*;
use crate::itree::{ITreeNode, Interval};

use std::io::Write;

impl ITreeNode {
    /// Write an interval tree node
    pub fn to_writer<W: Write>(&self, w: &mut W) -> JifResult<usize> {
        let mut written = 0;
        for interval in self.ranges() {
            written += interval.to_writer(w)?;
        }

        Ok(written)
    }
}

impl Interval {
    /// Write an interval
    pub fn to_writer<W: Write>(&self, w: &mut W) -> JifResult<usize> {
        w.write_all(&self.start.to_le_bytes())?;
        w.write_all(&self.end.to_le_bytes())?;
        w.write_all(&self.offset.to_le_bytes())?;
        Ok(Interval::serialized_size())
    }
}
