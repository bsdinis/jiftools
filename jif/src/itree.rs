//! The interval tree structure

use crate::deduper::Deduper;
use crate::error::*;
use crate::interval::{AnonIntervalData, DataSource, Interval, IntervalData, RefIntervalData};
use crate::itree_node::{ITreeNode, FANOUT};
use crate::utils::PAGE_SIZE;

/// Generic view over the two possible types of [`ITree`]
pub enum ITreeView<'a> {
    /// Anonymous [`ITree`]
    Anon { inner: &'a ITree<AnonIntervalData> },

    /// Reference [`ITree`]
    Ref { inner: &'a ITree<RefIntervalData> },
}

impl<'a> ITreeView<'a> {
    /// Size of the [`ITree`] in number of nodes
    pub fn n_nodes(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.n_nodes(),
            ITreeView::Ref { inner } => inner.n_nodes(),
        }
    }

    /// Size of the [`ITree`] in number of intervals
    pub fn n_intervals(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.n_intervals(),
            ITreeView::Ref { inner } => inner.n_intervals(),
        }
    }

    /// Number of intervals holding data
    pub fn n_data_intervals(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.n_data_intervals(),
            ITreeView::Ref { inner } => inner.n_data_intervals(),
        }
    }

    /// Size of _explicit_ mappings to the zero page
    pub fn zero_byte_size(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.zero_byte_size(),
            ITreeView::Ref { inner } => inner.zero_byte_size(),
        }
    }

    /// Size of mappings to data
    pub fn private_data_size(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.private_data_size(),
            ITreeView::Ref { inner } => inner.private_data_size(),
        }
    }

    /// Iterate over the private pages in the interval tree
    pub fn iter_private_pages(
        &'a self,
        deduper: &'a Deduper,
    ) -> Box<dyn Iterator<Item = &[u8]> + 'a> {
        match self {
            ITreeView::Anon { inner } => Box::new(inner.iter_private_pages(deduper)),
            ITreeView::Ref { inner } => Box::new(inner.iter_private_pages(deduper)),
        }
    }

    /// Resolve address in the interval tree
    pub fn resolve(&self, addr: u64) -> DataSource {
        match self {
            ITreeView::Anon { inner } => inner
                .resolve(addr)
                .map(|ival| (&ival.data).into())
                .unwrap_or(DataSource::Zero),
            ITreeView::Ref { inner } => inner
                .resolve(addr)
                .map(|ival| (&ival.data).into())
                .unwrap_or(DataSource::Shared),
        }
    }
}

/// Interval Tree representation
///
/// A balanced B-Tree where each node resolves an interval into a "data source".
///
/// Depending on the generic [`IntervalData`] parameter, the tree can either be "anonymous" or
/// "reference" if it is associated with an anonymous VMA or file-backed VMA, respectively.
///
/// For an **anonymous** virtual address range the tree is meant to span,
/// looking up an address can yield 2 options
///  - Address is found, with a valid offset: means the address is backed by the page at that offset of the JIF file
///  - Address is not found: means the address is backed by the zero page
///
/// For a **file-backed** virtual address range the tree is meant to span,
/// looking up an address can yield 3 options
///  - Address is found, with offset `u64::MAX`: means the address is backed by the zero page
///  - Address is found, with a valid offset: means the address is backed by the page at that offset of the JIF file
///  - Address is not found: means the address is backed by the reference file (with the offset
///  being the offset of the virtual address into the virtual address range)
///
pub struct ITree<Data: IntervalData> {
    pub(crate) nodes: Vec<ITreeNode<Data>>,
}

impl<Data: IntervalData + std::default::Default> ITree<Data> {
    /// Construct a new interval tree
    pub fn new(nodes: Vec<ITreeNode<Data>>, virtual_range: (u64, u64)) -> JifResult<Self> {
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

        if (virtual_range.1 - virtual_range.0) as usize
            != covered_by_zero + covered_by_private + non_mapped
        {
            return Err(JifError::InvalidITree {
                virtual_range,
                error: ITreeError::RangeNotCovered {
                    expected_coverage: (virtual_range.1 - virtual_range.0) as usize,
                    covered_by_zero,
                    covered_by_private,
                    non_mapped,
                },
            });
        }

        Ok(ITree { nodes })
    }

    /// Take ownership of the [`ITree`], leaving it empty
    pub fn take(&mut self) -> Self {
        let nodes = self.nodes.split_off(0);
        ITree { nodes }
    }

    /// How many [`ITreeNode`]s will be required given the number of intervals
    pub const fn n_itree_nodes_from_intervals(n_intervals: usize) -> usize {
        (n_intervals + FANOUT - 2) / (FANOUT - 1)
    }

    /// Build a new interval tree (by balancing the input [`Interval`]s)
    pub fn build(mut intervals: Vec<Interval<Data>>, virtual_range: (u64, u64)) -> JifResult<Self> {
        fn fill<Data: IntervalData>(
            nodes: &mut Vec<ITreeNode<Data>>,
            intervals: &mut Vec<Interval<Data>>,
            node_idx: usize,
        ) {
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
        ITree::new(nodes, virtual_range)
    }

    /// Size of the [`ITree`] in number of nodes
    pub fn n_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Number of intervals in the [`ITree`]
    pub fn n_intervals(&self) -> usize {
        self.nodes.iter().map(|n| n.n_intervals()).sum()
    }

    /// Number of data holding intervals in the [`ITree`]
    pub fn n_data_intervals(&self) -> usize {
        self.nodes.iter().map(|n| n.n_data_intervals()).sum()
    }

    /// Iterate over the intervals
    pub(crate) fn into_iter_intervals(self) -> impl Iterator<Item = Interval<Data>> {
        self.nodes.into_iter().flat_map(|n| n.ranges.into_iter())
    }
    /// Iterate over the intervals
    pub(crate) fn iter_intervals(&self) -> impl Iterator<Item = &Interval<Data>> {
        self.nodes.iter().flat_map(|n| n.ranges.iter())
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

    /// Iterate over the private pages in the interval tree
    ///
    // TODO(array_chunks): waiting on the `array_chunks` (#![feature(iter_array_chunks)]) that carries
    // the size information to change the output type to &[u8; PAGE_SIZE]
    pub fn iter_private_pages<'a>(
        &'a self,
        deduper: &'a Deduper,
    ) -> impl Iterator<Item = &[u8]> + 'a {
        ITreeIterator::new(self)
            .filter_map(|i| i.data.get_data(deduper).map(|d| d.chunks_exact(PAGE_SIZE)))
            .flatten()
    }

    pub fn resolve(&self, addr: u64) -> Option<&Interval<Data>> {
        fn resolve_aux<Data: IntervalData>(
            nodes: &[ITreeNode<Data>],
            addr: u64,
            node_idx: usize,
        ) -> Option<&Interval<Data>> {
            // base case: over len
            if node_idx >= nodes.len() {
                return None;
            }

            let child_idx = |i| node_idx * FANOUT + 1 + i;
            for (idx, node) in nodes[node_idx].ranges.iter().enumerate() {
                match node.cmp(addr) {
                    std::cmp::Ordering::Less => {
                        return resolve_aux(nodes, addr, child_idx(idx));
                    }
                    std::cmp::Ordering::Equal => {
                        return Some(node);
                    }
                    std::cmp::Ordering::Greater => {
                        // not found, continue
                    }
                }
            }

            resolve_aux(nodes, addr, child_idx(FANOUT - 1))
        }

        resolve_aux(&self.nodes, addr, 0 /* node_idx */)
    }
}

#[derive(Clone, Copy, Debug)]
enum InOrderTraversalState {
    Outer {
        node_idx: usize,
    },
    BeforeRecursion {
        node_idx: usize,
        child_idx: usize,
        range_idx: usize,
    },
    AfterRecursion {
        node_idx: usize,
        child_idx: usize,
        range_idx: usize,
    },
}
struct ITreeIterator<'a, Data: IntervalData> {
    nodes: &'a [ITreeNode<Data>],
    stack: Vec<InOrderTraversalState>,
}

impl<'a, Data: IntervalData> ITreeIterator<'a, Data> {
    fn new(itree: &'a ITree<Data>) -> Self {
        let stack = if itree.nodes.is_empty() {
            Vec::new()
        } else {
            vec![InOrderTraversalState::Outer { node_idx: 0 }]
        };
        ITreeIterator {
            nodes: &itree.nodes,
            stack,
        }
    }
}

impl<'a, Data: IntervalData> Iterator for ITreeIterator<'a, Data> {
    type Item = &'a Interval<Data>;
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(state) = self.stack.pop() {
            match state {
                InOrderTraversalState::Outer { node_idx } => {
                    if node_idx < self.nodes.len() {
                        self.stack.push(InOrderTraversalState::BeforeRecursion {
                            node_idx,
                            child_idx: node_idx * FANOUT + 1,
                            range_idx: 0,
                        });
                    }
                }
                InOrderTraversalState::BeforeRecursion {
                    node_idx,
                    child_idx,
                    range_idx,
                } => {
                    if range_idx < FANOUT - 1 {
                        self.stack.push(InOrderTraversalState::AfterRecursion {
                            node_idx,
                            child_idx,
                            range_idx,
                        });
                    }

                    if range_idx < FANOUT {
                        self.stack.push(InOrderTraversalState::Outer {
                            node_idx: child_idx,
                        })
                    }
                }
                InOrderTraversalState::AfterRecursion {
                    node_idx,
                    child_idx,
                    range_idx,
                } => {
                    self.stack.push(InOrderTraversalState::BeforeRecursion {
                        node_idx,
                        child_idx: child_idx + 1,
                        range_idx: range_idx + 1,
                    });

                    return Some(&self.nodes[node_idx].ranges[range_idx]);
                }
            }
        }

        None
    }
}

impl<Data: IntervalData + std::fmt::Debug> std::fmt::Debug for ITree<Data> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.nodes.iter()).finish()
    }
}

impl<'a> std::fmt::Debug for ITreeView<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ITreeView::Anon { inner } => inner.fmt(f),
            ITreeView::Ref { inner } => inner.fmt(f),
        }
    }
}
