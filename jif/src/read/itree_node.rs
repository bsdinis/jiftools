use crate::error::*;
use crate::itree::interval::RawInterval;
use crate::itree::itree_node::{RawITreeNode, IVAL_PER_NODE};

use std::io::Read;

impl RawITreeNode {
    /// Read and parse an RawITreeNode
    pub fn from_reader<R: Read>(r: &mut R) -> ITreeNodeResult<Self> {
        let mut ranges = [RawInterval::default(); IVAL_PER_NODE];
        for (interval_idx, interval) in ranges.iter_mut().enumerate() {
            *interval =
                RawInterval::from_reader(r).map_err(|interval_err| ITreeNodeError::Interval {
                    interval_idx,
                    interval_err,
                })?;
        }

        Ok(RawITreeNode::new(ranges))
    }
}
