use crate::error::*;
use crate::interval::RawInterval;
use crate::itree_node::{RawITreeNode, IVAL_PER_NODE};

use std::io::Read;

impl RawITreeNode {
    /// Read and parse an RawITreeNode
    pub fn from_reader<R: Read>(r: &mut R, itree_node_idx: usize) -> JifResult<Self> {
        let mut ranges = [RawInterval::default(); IVAL_PER_NODE];
        for (idx, interval) in ranges.iter_mut().enumerate() {
            *interval = RawInterval::from_reader(r, itree_node_idx, idx)?;
        }

        Ok(RawITreeNode::new(ranges))
    }
}
