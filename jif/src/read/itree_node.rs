use crate::error::*;
use crate::itree_node::{RawITreeNode, RawInterval, IVAL_PER_NODE};

use crate::utils::{is_page_aligned, read_u64};
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

impl RawInterval {
    /// Read and parse a RawInterval
    pub fn from_reader<R: Read>(
        r: &mut R,
        itree_node_idx: usize,
        interval_idx: usize,
    ) -> JifResult<Self> {
        fn read_page_aligned_u64<R: Read>(
            r: &mut R,
            buffer: &mut [u8; 8],
            itree_node_idx: usize,
            interval_idx: usize,
        ) -> JifResult<u64> {
            let v = read_u64(r, buffer)?;

            // MAX is a special value
            if v == u64::MAX {
                return Ok(v);
            }

            if !is_page_aligned(v) {
                Err(JifError::BadITreeNode {
                    itree_node_idx,
                    itree_node_err: ITreeNodeError {
                        interval_idx,
                        interval_err: IntervalError::BadAlignment(v),
                    },
                })
            } else {
                Ok(v)
            }
        }

        let mut buffer = [0u8; 8];

        let start = read_page_aligned_u64(r, &mut buffer, itree_node_idx, interval_idx)?;
        let end = read_page_aligned_u64(r, &mut buffer, itree_node_idx, interval_idx)?;
        let offset = read_page_aligned_u64(r, &mut buffer, itree_node_idx, interval_idx)?;

        if start == u64::MAX || end == u64::MAX {
            if start == end && offset == u64::MAX {
                // this is a default Interval
                return Ok(RawInterval::default());
            } else {
                return Err(JifError::BadITreeNode {
                    itree_node_idx,
                    itree_node_err: ITreeNodeError {
                        interval_idx,
                        interval_err: IntervalError::InvalidInterval(start, end, offset),
                    },
                });
            }
        }

        if start > end {
            return Err(JifError::BadITreeNode {
                itree_node_idx,
                itree_node_err: ITreeNodeError {
                    interval_idx,
                    interval_err: IntervalError::BadRange(start, end),
                },
            });
        }

        Ok(RawInterval::new(start, end, offset))
    }
}
