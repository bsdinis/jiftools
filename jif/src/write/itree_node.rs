use crate::itree::itree_node::RawITreeNode;

use std::io::Write;

impl RawITreeNode {
    /// Write an interval tree node
    pub fn to_writer<W: Write>(&self, w: &mut W) -> std::io::Result<usize> {
        let mut written = 0;
        for interval in self.ranges() {
            written += interval.to_writer(w)?;
        }

        Ok(written)
    }
}
