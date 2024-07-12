use crate::error::*;
use crate::itree_node::{ITreeNode, Interval, FANOUT};

/// Interval Tree representation
///
/// A balanced B-Tree where each node resolves an interval
/// into a "data source".
///
/// For the virtual address range the tree is meant to span,
/// looking up an address can yield 3 options
///  - Address is found, with offset `u64::MAX`: means the address is backed by the zero page
///  - Address is found, with a valid offset: means the address is backed by the page at that offset of the JIF file
///  - Address is not found: means the address is backed by the reference file (with the offset
///  being the offset of the virtual address into the virtual address range)
///
pub struct ITree {
    pub(crate) nodes: Vec<ITreeNode>,
}

impl ITree {
    /// Construct a new interval tree
    pub fn new(
        nodes: Vec<ITreeNode>,
        virtual_range: (u64, u64),
        has_backing_reference: bool,
    ) -> JifResult<Self> {
        let intervals = {
            let mut i = nodes
                .iter()
                .flat_map(|n| n.ranges.iter())
                .filter(|i| !i.is_none())
                .map(|x| (x.start, x.end))
                .collect::<Vec<_>>();
            i.sort_by_key(|x| x.0);
            i
        };

        if let Some(interval) = intervals
            .iter()
            .find(|(start, end)| *start < virtual_range.0 || *end > virtual_range.1)
            .copied()
        {
            return Err(JifError::InvalidITree {
                virtual_range,
                error: ITreeError::IntervalOutOfRange { interval },
            });
        }

        if let Some((interval_1, interval_2)) = intervals
            .iter()
            .zip(intervals.iter().skip(1))
            .find(|((_, end), (start, _))| end > start)
        {
            return Err(JifError::InvalidITree {
                virtual_range,
                error: ITreeError::IntersectingInterval {
                    interval_1: *interval_1,
                    interval_2: *interval_2,
                },
            });
        }

        let covered_by_zero = nodes.iter().map(|n| n.zero_byte_size()).sum::<usize>();
        let covered_by_private = nodes.iter().map(|n| n.private_data_size()).sum::<usize>();
        let non_mapped = {
            let create_iter = || {
                std::iter::once((0u64, virtual_range.0))
                    .chain(intervals.iter().copied())
                    .chain(std::iter::once((virtual_range.1, u64::MAX)))
            };

            create_iter()
                .zip(create_iter().skip(1))
                .map(|((_, end), (start, _))| (start - end) as usize)
                .sum::<usize>()
        };

        if has_backing_reference {
            if (virtual_range.1 - virtual_range.0) as usize
                != covered_by_zero + covered_by_private + non_mapped
            {
                return Err(JifError::InvalidITree {
                    virtual_range,
                    error: ITreeError::ReferenceNotCovered {
                        expected_coverage: (virtual_range.1 - virtual_range.0) as usize,
                        covered_by_zero,
                        covered_by_private,
                        non_mapped,
                    },
                });
            }
        } else if (virtual_range.1 - virtual_range.0) as usize
            != covered_by_zero + covered_by_private
        {
            return Err(JifError::InvalidITree {
                virtual_range,
                error: ITreeError::NonReferenceNotCovered {
                    expected_coverage: (virtual_range.1 - virtual_range.0) as usize,
                    covered_by_zero,
                    covered_by_private,
                },
            });
        } else if non_mapped > 0 {
            return Err(JifError::InvalidITree {
                virtual_range,
                error: ITreeError::NonReferenceHoled { non_mapped },
            });
        }

        Ok(ITree { nodes })
    }

    pub fn take(&mut self) -> Self {
        let nodes = self.nodes.split_off(0);
        ITree { nodes }
    }

    /// How many itree nodes will be required given the number of intervals
    pub const fn n_itree_nodes_from_intervals(n_intervals: usize) -> usize {
        (n_intervals + FANOUT - 2) / (FANOUT - 1)
    }

    /// Build a new interval tree (by balancing the intervals)
    pub fn build(
        mut intervals: Vec<Interval>,
        virtual_range: (u64, u64),
        has_reference: bool,
    ) -> JifResult<Self> {
        fn fill(nodes: &mut Vec<ITreeNode>, intervals: &mut Vec<Interval>, node_idx: usize) {
            // first base case: no node with this index
            if node_idx >= nodes.len() {
                return;
            }

            let mut child_idx = node_idx * FANOUT + 1;
            for i in 0..(FANOUT - 1) {
                // recursion
                fill(nodes, intervals, child_idx);

                if let Some(interval) = intervals.pop() {
                    // insert an interval
                    nodes[node_idx].ranges[i] = interval;
                    child_idx += 1;
                } else {
                    // second base case: no more intervals
                    return;
                }
            }

            // FANOUT == IVAL_PER_NODE - 1, so we need to insert right_most child
            fill(nodes, intervals, child_idx);
        }

        let n_nodes = Self::n_itree_nodes_from_intervals(intervals.len());
        let mut nodes = (0..n_nodes)
            .map(|_| ITreeNode::default())
            .collect::<Vec<_>>();

        // sort intervals in descending order of start (we pop them out the back)
        intervals.sort_by(|it1, it2| it2.start.cmp(&it1.start));
        fill(&mut nodes, &mut intervals, 0);
        ITree::new(nodes, virtual_range, has_reference)
    }

    /// Size of the interval tree in number of nodes
    pub fn n_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Number of intervals in the inteval tree
    pub fn n_intervals(&self) -> usize {
        self.nodes.iter().map(|n| n.n_intervals()).sum()
    }

    /// Number of data holding intervals in the inteval tree
    pub fn n_data_intervals(&self) -> usize {
        self.nodes.iter().map(|n| n.n_data_intervals()).sum()
    }

    /// Iterate over the intervals
    pub(crate) fn into_iter_intervals(self) -> impl Iterator<Item = Interval> {
        self.nodes.into_iter().flat_map(|n| n.ranges.into_iter())
    }
    /// Iterate over the intervals
    pub(crate) fn iter_intervals(&self) -> impl Iterator<Item = &Interval> {
        self.nodes.iter().flat_map(|n| n.ranges.iter())
    }
    /// Mutably Iterate over the intervals
    pub(crate) fn iter_mut_intervals(&mut self) -> impl Iterator<Item = &mut Interval> {
        self.nodes.iter_mut().flat_map(|n| n.ranges.iter_mut())
    }

    /// How much of the interval tree consists of zero page mappings
    pub fn zero_byte_size(&self) -> usize {
        self.nodes.iter().map(ITreeNode::zero_byte_size).sum()
    }

    /// How much of the interval tree consists of private page mappings (i.e., data in the JIF)
    pub fn private_data_size(&self) -> usize {
        self.nodes.iter().map(ITreeNode::private_data_size).sum()
    }

    /// How much of a particular `[start; end)` sub-interval of the address space
    /// does this interval tree map with either zero pages or private pages
    pub(crate) fn mapped_subregion_size(&self, start: u64, end: u64) -> usize {
        self.nodes
            .iter()
            .map(|n| n.mapped_subregion_size(start, end))
            .sum()
    }

    /// How much of a particular `[start; end)` sub-interval of the address space
    /// does this interval tree not map (i.e., will be backed by a reference segment)
    pub(crate) fn not_mapped_subregion_size(&self, start: u64, end: u64) -> usize {
        (end - start) as usize - self.mapped_subregion_size(start, end)
    }
}

impl std::fmt::Debug for ITree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.nodes.iter()).finish()
    }
}
