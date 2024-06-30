use crate::error::*;
use crate::utils::{is_page_aligned, read_u64};

use std::io::Read;

const FANOUT: usize = 4;
const IVAL_PER_NODE: usize = FANOUT - 1;

pub struct ITree {
    nodes: Vec<ITreeNode>,
}

#[derive(Clone)]
pub struct ITreeNode {
    ranges: [Interval; IVAL_PER_NODE],
}

#[derive(Default, Clone, Copy)]
pub(crate) struct Interval {
    start: u64,
    end: u64,
    offset: u64,
}

impl ITree {
    pub fn new(nodes: Vec<ITreeNode>) -> Self {
        ITree { nodes }
    }
}

impl ITreeNode {
    pub(crate) const fn serialized_size() -> usize {
        IVAL_PER_NODE * Interval::serialized_size()
    }

    pub fn from_reader<R: Read>(r: &mut R, itree_node_idx: usize) -> JifResult<Self> {
        let mut ranges = [Interval::default(); IVAL_PER_NODE];
        for idx in 0..IVAL_PER_NODE {
            // TODO: remove unwrap or else
            ranges[idx] = Interval::from_reader(r, itree_node_idx, idx).unwrap_or(Interval {
                start: u64::MAX,
                end: u64::MAX,
                offset: u64::MAX,
            });
        }

        Ok(ITreeNode { ranges })
    }
}

impl Interval {
    pub(crate) const fn serialized_size() -> usize {
        3 * std::mem::size_of::<u64>()
    }

    fn valid(&self) -> bool {
        self.start != u64::MAX && self.end != u64::MAX && self.offset != u64::MAX
    }

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

        Ok(Interval { start, end, offset })
    }
}

impl std::fmt::Debug for ITree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.nodes.iter()).finish()
    }
}

impl std::fmt::Debug for ITreeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ITreeNode: ")?;
        f.debug_list()
            .entries(self.ranges.iter().filter(|i| i.valid()))
            .finish()
    }
}

impl std::fmt::Debug for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.valid() {
            f.debug_struct("EmptyInterval").finish()
        } else {
            f.write_fmt(format_args!(
                "[{:#x}; {:#x}) -> {:#x}",
                &self.start, &self.end, &self.offset
            ))
        }
    }
}
