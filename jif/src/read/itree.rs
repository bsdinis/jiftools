use crate::error::*;
use crate::itree::{ITreeNode, Interval, IVAL_PER_NODE};

use crate::utils::{is_page_aligned, read_u64};
use std::io::Read;

impl ITreeNode {
    pub fn from_reader<R: Read>(r: &mut R, itree_node_idx: usize) -> JifResult<Self> {
        let mut ranges = [Interval::default(); IVAL_PER_NODE];
        for idx in 0..IVAL_PER_NODE {
            ranges[idx] =
                Interval::from_reader(r, itree_node_idx, idx).unwrap_or(Interval::default());
        }

        Ok(ITreeNode::new(ranges))
    }
}

impl Interval {
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

        if (start == u64::MAX || end == u64::MAX || offset == u64::MAX)
            && (start != end || end != offset)
        {
            return Err(JifError::BadITreeNode {
                itree_node_idx,
                itree_node_err: ITreeNodeError {
                    interval_idx,
                    interval_err: IntervalError::InvalidInterval(start, end, offset),
                },
            });
        }

        if start >= end {
            return Err(JifError::BadITreeNode {
                itree_node_idx,
                itree_node_err: ITreeNodeError {
                    interval_idx,
                    interval_err: IntervalError::BadRange(start, end),
                },
            });
        }

        Ok(Interval::new(start, end, offset))
    }
}
