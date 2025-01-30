//! The interval tree structure

use std::collections::HashSet;

use crate::deduper::{DedupToken, Deduper};
use crate::error::*;
use crate::itree::interval::{DataSource, Interval, IntervalData};
use crate::itree::itree_node::{ITreeNode, FANOUT};
use crate::ord::OrdChunk;
use crate::utils::PAGE_SIZE;

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
///  - Address is not found: means the address is backed by the reference file (with the offset being the offset of the virtual address into the virtual address range)
///
pub struct ITree<Data: IntervalData> {
    pub(crate) nodes: Vec<ITreeNode<Data>>,
    virtual_range: (u64, u64),
}

impl<Data: IntervalData + std::default::Default> ITree<Data> {
    /// Construct a new interval tree
    pub fn new(nodes: Vec<ITreeNode<Data>>, virtual_range: (u64, u64)) -> ITreeResult<Self> {
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
            return Err(ITreeError::IntervalOutOfRange { interval });
        }

        if let Some((interval_1, interval_2)) = intervals
            .iter()
            .zip(intervals.iter().skip(1))
            .find(|((_, end), (start, _))| end > start)
        {
            return Err(ITreeError::IntersectingInterval {
                interval_1: *interval_1,
                interval_2: *interval_2,
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
            return Err(ITreeError::RangeNotCovered {
                expected_coverage: (virtual_range.1 - virtual_range.0) as usize,
                covered_by_zero,
                covered_by_private,
                non_mapped,
            });
        }

        let itree = ITree {
            nodes,
            virtual_range,
        };

        let expected_n_nodes = Self::n_itree_nodes_from_intervals(itree.n_intervals());
        if expected_n_nodes < itree.n_nodes() {
            return Err(ITreeError::NotCompact {
                expected_n_nodes,
                n_nodes: itree.n_nodes(),
            });
        }

        if let Some((i1, i2)) = itree
            .in_order_intervals()
            .zip(itree.in_order_intervals().skip(1))
            .find(|(i1, i2)| i1.end > i2.start)
        {
            return Err(ITreeError::NotInOrder {
                interval_1: (i1.start, i1.end),
                interval_2: (i2.start, i2.end),
            });
        }

        Ok(itree)
    }

    pub fn single(virtual_range: (u64, u64), data: Data) -> Self {
        ITree {
            nodes: vec![ITreeNode::single(Interval {
                start: virtual_range.0,
                end: virtual_range.1,
                data,
            })],
            virtual_range,
        }
    }

    pub fn single_default(virtual_range: (u64, u64)) -> Self {
        ITree {
            nodes: vec![],
            virtual_range,
        }
    }

    /// Take ownership of the [`ITree`], leaving it empty
    pub fn take(&mut self) -> Self {
        let nodes = self.nodes.split_off(0);
        ITree {
            nodes,
            virtual_range: self.virtual_range,
        }
    }

    /// Fracture data intervals in ITree by the ordering section boundaries
    /// to ensure they can be reordered in the physical representation
    /// (to be more efficiently placed in the file)
    pub fn fracture(&mut self, ord_chunks: &[OrdChunk], deduper: &Deduper) -> JifResult<()> {
        fn fracture_interval<Data: IntervalData>(
            ival: &mut Interval<Data>,
            chunk: &OrdChunk,
            deduper: &Deduper,
            intervals: &mut Vec<Interval<Data>>,
            intervals_to_recheck: &mut Vec<Interval<Data>>,
        ) {
            let mut ival_data = if ival.data.is_owned() {
                ival.data.take_data().unwrap()
            } else {
                assert!(ival.data.is_ref());
                ival.data.get_data(deduper).unwrap().to_vec()
            };

            // breaking an interval where the ordering section starts in the middle
            let mut ival_data = if chunk.vaddr > ival.start {
                let remainder = ival_data.split_off((chunk.vaddr - ival.start) as usize);

                intervals.push(Interval {
                    start: ival.start,
                    end: chunk.vaddr,
                    data: ival_data.into(),
                });

                remainder
            } else {
                ival_data
            };

            if chunk.end() == ival.end {
                // if the ordering section is lined at the end
                intervals.push(Interval {
                    start: chunk.vaddr,
                    end: ival.end,
                    data: ival_data.into(),
                });
            } else {
                // this is a tricky case: we are breaking an interval that has leftover
                // data to the right: this means there could be another ordering chunk
                // that could fracture the interval, but we cannot push it again into
                // the old_intervals vector.
                //
                // So we push it into its own vector to be checked at a later stage

                let remainder = ival_data.split_off((chunk.end() - chunk.vaddr) as usize);
                intervals.push(Interval {
                    start: chunk.vaddr,
                    end: chunk.end(),
                    data: ival_data.into(),
                });

                assert!(chunk.end() < ival.end);
                intervals_to_recheck.push(Interval {
                    start: chunk.end(),
                    end: ival.end,
                    data: remainder.into(),
                });
            }
        }

        let virt_range = self.virtual_range();

        let mut old_intervals = self
            .nodes
            .iter_mut()
            .flat_map(|node| node.ranges.iter_mut())
            .filter(|x| !x.is_none())
            .collect::<Vec<_>>();

        // uninteresting pheader, skip
        if old_intervals.is_empty() {
            return Ok(());
        }

        let pertinent_chunks = {
            let mut v = ord_chunks
                .iter()
                .filter(|x| x.kind == DataSource::Private)
                .filter(|x| virt_range.0 <= x.vaddr && x.vaddr < virt_range.1)
                .inspect(|x| {
                    assert!(
                        x.end() <= virt_range.1,
                        "ord chunk [{:x?}-{:x?}] does not fit within the itree [{:x?}-{:x?}]",
                        x.vaddr,
                        x.end(),
                        virt_range.0,
                        virt_range.1
                    )
                })
                .collect::<Vec<_>>();

            v.sort_by_key(|x| x.vaddr);
            v
        };

        let mut intervals = vec![];
        let mut intervals_to_recheck = vec![];

        for chunk in pertinent_chunks {
            if old_intervals.is_empty() && intervals_to_recheck.is_empty() {
                // no more intervals to break
                break;
            }

            let mut old_ival_idx_to_remove = None;
            // check the old intervals

            if let Some((idx, ival)) = old_intervals
                .iter_mut()
                .enumerate()
                .filter(|(_idx, x)| x.is_data())
                .find(|(_idx, x)| x.start <= chunk.vaddr && chunk.vaddr < x.end)
            {
                assert!(chunk.end() <= ival.end, "found an ordering chunk [{:x?}-{:x?}] that spans multiple intervals: (first interval is [{:x?}-{:x?}]",
                    chunk.vaddr, chunk.end(), ival.start, ival.end);

                // if the interval matches with the entirety of the ordering chunk,
                // we shouldn't do anything
                if ival.start == chunk.vaddr && ival.end == chunk.end() {
                    continue;
                }

                old_ival_idx_to_remove = Some(idx);
                fracture_interval(
                    ival,
                    chunk,
                    deduper,
                    &mut intervals,
                    &mut intervals_to_recheck,
                );
            }

            if let Some(idx_to_remove) = old_ival_idx_to_remove {
                old_intervals.remove(idx_to_remove);
                continue;
            }

            let mut new_intervals = vec![];
            let mut idx_to_move_to_ivals = None;
            let mut idx_to_remove = None;

            if let Some((idx, ival)) = intervals_to_recheck
                .iter_mut()
                .enumerate()
                .find(|(_idx, x)| x.start <= chunk.vaddr && chunk.vaddr < x.end)
            {
                assert!(chunk.end() <= ival.end, "found an ordering chunk [{:x?}-{:x?}] that spans multiple intervals: (first interval is [{:x?}-{:x?}]",
                        chunk.vaddr, chunk.end(), ival.start, ival.end);
                assert!(
                    ival.is_data(),
                    "we shouldn't have to recheck intervals that aren't data"
                );

                if ival.start == chunk.vaddr && ival.end == chunk.end() {
                    idx_to_move_to_ivals = Some(idx);
                } else {
                    idx_to_remove = Some(idx);
                    fracture_interval(ival, chunk, deduper, &mut intervals, &mut new_intervals);
                }
            } else {
                panic!(
                    "failed to find a data interval for private-data backed orc {:#x?}",
                    chunk
                );
            }

            // case where the remaining interval matches perfectly with an existing ord chunk
            if let Some(idx) = idx_to_move_to_ivals {
                intervals.push(intervals_to_recheck.remove(idx));
            }
            if let Some(idx) = idx_to_remove {
                intervals_to_recheck.remove(idx);
            }

            intervals_to_recheck.append(&mut new_intervals);
        }

        intervals.append(&mut intervals_to_recheck);

        // at this point, we should check that all the intervals are owned data
        intervals.iter().for_each(|x| assert!(x.data.is_owned()));

        intervals.reserve(old_intervals.len());
        old_intervals
            .into_iter()
            .for_each(|x| intervals.push(x.clone()));

        for ival in self
            .nodes
            .iter()
            .flat_map(|node| node.ranges.iter())
            .filter(|x| x.is_data())
        {
            let mut bytes_found = 0;
            while bytes_found < ival.end - ival.start {
                if let Some(new_ival) = intervals
                    .iter()
                    .filter(|x| x.is_data())
                    .find(|x| x.start == ival.start + bytes_found)
                {
                    bytes_found += new_ival.len();
                } else {
                    panic!("lost data in the fracture: original interval [{:#x?}-{:#x?}] cannot find sub-interval [{:#x?}-{:#x?}] in new intervals",
                        ival.start, ival.end,
                        ival.start + bytes_found, ival.end);
                }
            }
        }

        intervals.iter().filter(|x| x.is_data()).for_each(|x| {
            assert!(x.data.get_data(deduper).unwrap().len() == (x.end - x.start) as usize)
        });

        *self = Self::build(intervals, self.virtual_range()).map_err(|error| {
            JifError::InvalidITree {
                virtual_range: self.virtual_range(),
                error,
            }
        })?;

        Ok(())
    }

    /// Bring owned data into the deduper
    pub fn dedup(&mut self, deduper: &mut Deduper) {
        self.nodes.iter_mut().for_each(|node| {
            node.ranges
                .iter_mut()
                .for_each(|interval| interval.data.dedup(deduper))
        })
    }

    /// Report tokens being used
    pub fn add_tokens_in_use(&self, tokens_in_use: &mut HashSet<DedupToken>) {
        self.nodes.iter().for_each(|node| {
            node.ranges().iter().for_each(|interval| {
                interval
                    .data
                    .dedup_token()
                    .map(|tok| tokens_in_use.insert(tok));
            })
        })
    }

    /// How many [`ITreeNode`]s will be required given the number of intervals
    pub const fn n_itree_nodes_from_intervals(n_intervals: usize) -> usize {
        (n_intervals + FANOUT - 2) / (FANOUT - 1)
    }

    /// Build a new interval tree (by balancing the input [`Interval`]s)
    pub fn build(
        mut intervals: Vec<Interval<Data>>,
        virtual_range: (u64, u64),
    ) -> ITreeResult<Self> {
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

    /// Virtual range spanned by the interval tree
    pub fn virtual_range(&self) -> (u64, u64) {
        self.virtual_range
    }

    /// Size of the [`ITree`] in number of nodes
    pub fn n_nodes(&self) -> usize {
        self.nodes.len()
    }

    pub fn n_intervals(&self) -> usize {
        let Some(first_explicit_addr) = self.in_order_intervals().next().map(|x| x.start) else {
            // empty itree
            return if self.virtual_range.0 == self.virtual_range.1 {
                0
            } else {
                1
            };
        };

        let Some(last_explicit_addr) = self.in_order_intervals().last().map(|x| x.end) else {
            // empty itree
            return if self.virtual_range.0 == self.virtual_range.1 {
                0
            } else {
                1
            };
        };

        let in_tree_intervals = self
            .in_order_intervals()
            .map(|x| (x.start, x.end))
            .zip(
                self.in_order_intervals()
                    .map(|x| (x.start, x.end))
                    .skip(1)
                    // dummy interval to make sure the last interval counts
                    .chain(std::iter::once((last_explicit_addr, last_explicit_addr))),
            )
            .map(|(i1, i2)| if i1.1 == i2.0 { 1 } else { 2 })
            .sum::<usize>();

        in_tree_intervals
            + if first_explicit_addr == self.virtual_range().0 {
                0
            } else {
                1
            }
            + if last_explicit_addr == self.virtual_range().1 {
                0
            } else {
                1
            }
    }

    /// Number of intervals in the [`ITree`]
    pub fn n_explicit_intervals(&self) -> usize {
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
    pub(crate) fn in_order_intervals(&self) -> impl Iterator<Item = &Interval<Data>> {
        ITreeIterator::new(self)
    }

    /// How much of the interval tree consists of zero page mappings
    pub fn zero_byte_size(&self) -> usize {
        self.nodes.iter().map(ITreeNode::zero_byte_size).sum()
    }

    /// How much of the interval tree consists of private page mappings (i.e., data in the JIF)
    pub fn private_data_size(&self) -> usize {
        self.nodes.iter().map(ITreeNode::private_data_size).sum()
    }

    /// How much of a particular `[start; end)` sub-interval of the address space does the interval
    /// tree explicitely map
    pub(crate) fn explicitely_mapped_subregion_size(&self, start: u64, end: u64) -> usize {
        self.nodes
            .iter()
            .map(|n| n.explicitely_mapped_subregion_size(start, end))
            .sum()
    }

    /// How much of a particular `[start; end)` sub-interval of the address space
    /// does this interval tree map implicitely
    pub(crate) fn implicitely_mapped_subregion_size(&self, start: u64, end: u64) -> usize {
        (end - start) as usize - self.explicitely_mapped_subregion_size(start, end)
    }

    /// Iterate over the private pages in the interval tree
    ///
    // TODO(array_chunks): waiting on the `array_chunks` (#![feature(iter_array_chunks)]) that carries
    // the size information to change the output type to &[u8; PAGE_SIZE]
    pub fn iter_private_pages<'a>(
        &'a self,
        deduper: &'a Deduper,
    ) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.in_order_intervals()
            .filter_map(|i| i.data.get_data(deduper).map(|d| d.chunks_exact(PAGE_SIZE)))
            .flatten()
    }

    /// Iterate over the unmapped regions (i.e., things that are backed by the shared files)
    pub fn iter_unmapped_regions(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        std::iter::once((0, self.virtual_range.0))
            .chain(self.in_order_intervals().map(|iv| (iv.start, iv.end)))
            .zip(
                self.in_order_intervals()
                    .map(|iv| (iv.start, iv.end))
                    .chain(std::iter::once((self.virtual_range.1, u64::MAX))),
            )
            .filter_map(
                |((_s1, e1), (s2, _e2))| {
                    if e1 < s2 {
                        Some((e1, s2))
                    } else {
                        None
                    }
                },
            )
    }

    /// Resolve an address in the interval tree, or into the gap in the interval tree it belongs to
    pub fn resolve(&self, addr: u64) -> Result<&Interval<Data>, (u64, u64)> {
        fn resolve_aux<Data: IntervalData>(
            nodes: &[ITreeNode<Data>],
            addr: u64,
            node_idx: usize,
            mut range: (u64, u64),
        ) -> Result<&Interval<Data>, (u64, u64)> {
            // base case: over len
            if node_idx >= nodes.len() {
                return Err(range);
            }

            let child_idx = |i| node_idx * FANOUT + 1 + i;
            for (idx, ival) in nodes[node_idx].ranges.iter().enumerate() {
                match ival.cmp(addr) {
                    std::cmp::Ordering::Less => {
                        return resolve_aux(
                            nodes,
                            addr,
                            child_idx(idx),
                            (range.0, std::cmp::min(range.1, ival.start)),
                        );
                    }
                    std::cmp::Ordering::Equal => {
                        return Ok(ival);
                    }
                    std::cmp::Ordering::Greater => {
                        range = (std::cmp::max(range.0, ival.end), range.1);
                        // not found, continue
                    }
                }
            }

            resolve_aux(nodes, addr, child_idx(FANOUT - 1), range)
        }

        resolve_aux(&self.nodes, addr, 0 /* node_idx */, self.virtual_range)
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

                    let ival = &self.nodes[node_idx].ranges[range_idx];
                    return (!ival.is_none()).then_some(ival);
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

#[cfg(test)]
pub(crate) mod test {
    use crate::itree::interval::{AnonIntervalData, RefIntervalData};

    use super::*;

    pub(crate) const VADDR_BEGIN: u64 = 0x100000;
    pub(crate) const VADDR_END: u64 = 0x200000;
    pub(crate) const VADDRS: [u64; 17] = [
        VADDR_BEGIN,
        0x110000,
        0x120000,
        0x130000,
        0x140000,
        0x150000,
        0x160000,
        0x170000,
        0x180000,
        0x190000,
        0x1a0000,
        0x1b0000,
        0x1c0000,
        0x1d0000,
        0x1e0000,
        0x1f0000,
        VADDR_END,
    ];

    pub(crate) fn gen_empty<Data: IntervalData + Default + std::fmt::Debug>() -> ITree<Data> {
        ITree::new(Vec::new(), (VADDR_BEGIN, VADDR_END)).unwrap()
    }

    pub(crate) fn gen_anon_data(cnt: &mut usize, interval_size: usize) -> AnonIntervalData {
        let data = match *cnt % 2 {
            // cnt + 1 because we don't want accidental zeros
            0 => AnonIntervalData::Owned(vec![*cnt as u8 + 1; interval_size]),
            1 => AnonIntervalData::None,
            _ => std::unreachable!("mod 2 = [0, 1]"),
        };

        *cnt += 1;
        data
    }

    pub(crate) fn gen_anon_tree() -> ITree<AnonIntervalData> {
        let mut interval_cnt = 0;
        let intervals = VADDRS
            .iter()
            .copied()
            .zip(VADDRS.iter().copied().skip(1))
            .filter_map(|(start, end)| {
                let data = gen_anon_data(&mut interval_cnt, (end - start) as usize);
                if data.is_none() {
                    None
                } else {
                    Some(Interval { start, end, data })
                }
            })
            .collect();

        ITree::build(intervals, (VADDR_BEGIN, VADDR_END)).unwrap()
    }

    pub(crate) fn gen_ref_data(cnt: &mut usize, interval_size: usize) -> RefIntervalData {
        let data = match *cnt % 3 {
            // cnt + 1 because we don't want accidental zeros
            0 => RefIntervalData::Owned(vec![*cnt as u8 + 1; interval_size]),
            1 => RefIntervalData::Zero,
            2 => RefIntervalData::None,
            _ => std::unreachable!("mod 3 = [0, 1, 2]"),
        };

        *cnt += 1;
        data
    }

    pub(crate) fn gen_ref_tree() -> ITree<RefIntervalData> {
        let mut interval_cnt = 0;
        let intervals = VADDRS
            .iter()
            .copied()
            .zip(VADDRS.iter().copied().skip(1))
            .filter_map(|(start, end)| {
                let data = gen_ref_data(&mut interval_cnt, (end - start) as usize);
                if data.is_none() {
                    None
                } else {
                    Some(Interval { start, end, data })
                }
            })
            .collect();

        ITree::build(intervals, (VADDR_BEGIN, VADDR_END)).unwrap()
    }

    fn test_empty<Data: IntervalData>() {
        let tree: ITree<RefIntervalData> = gen_empty();
        assert_eq!(tree.n_nodes(), 0);
        assert_eq!(tree.n_explicit_intervals(), 0);
        assert_eq!(tree.n_data_intervals(), 0);
        assert_eq!(tree.in_order_intervals().count(), 0);
        assert_eq!(tree.zero_byte_size(), 0);
        assert_eq!(tree.private_data_size(), 0);
        assert_eq!(
            tree.explicitely_mapped_subregion_size(VADDR_BEGIN, VADDR_END),
            0
        );
        assert_eq!(
            tree.implicitely_mapped_subregion_size(VADDR_BEGIN, VADDR_END),
            (VADDR_END - VADDR_BEGIN) as usize
        );
        let deduper = Deduper::default();
        assert_eq!(tree.iter_private_pages(&deduper).count(), 0);
        assert_eq!(tree.resolve(0), Err((VADDR_BEGIN, VADDR_END)));
        assert_eq!(tree.resolve(VADDR_BEGIN), Err((VADDR_BEGIN, VADDR_END)));
        assert_eq!(
            tree.resolve((VADDR_BEGIN + VADDR_END) / 2),
            Err((VADDR_BEGIN, VADDR_END))
        );
        assert_eq!(tree.resolve(VADDR_END), Err((VADDR_BEGIN, VADDR_END)));
    }

    #[test]
    fn test_empty_anon() {
        test_empty::<AnonIntervalData>()
    }
    #[test]
    fn test_empty_ref() {
        test_empty::<RefIntervalData>()
    }

    #[test]
    fn test_anon_tree() {
        let tree = gen_anon_tree();
        let mut cnt = 0;
        // ranges are mapped on and off
        // we query the midpoint in each range
        for range in VADDRS.into_iter().zip(VADDRS.into_iter().skip(1)) {
            let addr = (range.0 + range.1) / 2;
            let resolve = tree.resolve(addr);
            match cnt % 2 {
                0 => assert!(matches!(
                    &resolve.unwrap().data,
                    &AnonIntervalData::Owned(_)
                )),
                1 => assert_eq!(resolve.unwrap_err(), range),
                _ => unreachable!(),
            };
            cnt += 1
        }

        // test the in order traversal is in order
        for (i1, i2) in tree
            .in_order_intervals()
            .zip(tree.in_order_intervals().skip(1))
        {
            assert!(i1.end <= i2.start);
        }

        {
            assert_eq!(
                tree.implicitely_mapped_subregion_size(VADDR_BEGIN, VADDR_END),
                std::iter::once((0, VADDR_BEGIN))
                    .chain(tree.in_order_intervals().map(|ival| (ival.start, ival.end)))
                    .zip(
                        tree.in_order_intervals()
                            .map(|ival| (ival.start, ival.end))
                            .chain(std::iter::once((VADDR_END, u64::MAX)))
                    )
                    .map(|((_s1, e1), (s2, _e2))| s2 as usize - e1 as usize)
                    .sum()
            );
        }
    }

    #[test]
    fn test_ref_tree() {
        let tree = gen_ref_tree();
        let mut cnt = 0;
        // ranges are mapped in an Owned -> Zero -> Ref cycle (Ref is implied)
        // we query the midpoint in each range
        for range in VADDRS.into_iter().zip(VADDRS.into_iter().skip(1)) {
            let addr = (range.0 + range.1) / 2;
            let resolve = tree.resolve(addr);
            match cnt % 3 {
                0 => assert!(matches!(&resolve.unwrap().data, &RefIntervalData::Owned(_))),
                1 => assert!(matches!(&resolve.unwrap().data, &RefIntervalData::Zero)),
                2 => assert_eq!(resolve.unwrap_err(), range),
                _ => unreachable!(),
            };
            cnt += 1
        }

        // test the in order traversal is in order
        for (i1, i2) in tree
            .in_order_intervals()
            .zip(tree.in_order_intervals().skip(1))
        {
            assert!(i1.end <= i2.start);
        }
    }
}
