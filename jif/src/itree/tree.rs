//! The interval tree structure

use std::collections::HashSet;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use rayon::prelude::*;

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

impl<Data: IntervalData + std::default::Default + Send> ITree<Data> {
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
    pub fn fracture(
        &mut self,
        ord_chunks: &[OrdChunk],
        deduper: Arc<RwLock<Deduper>>,
    ) -> JifResult<()> {
        fn fracture_interval<Data: IntervalData + Send>(
            mut ival: Interval<Data>,
            chunks: &[OrdChunk],
            deduper: Arc<RwLock<Deduper>>,
        ) -> Vec<Interval<Data>> {
            let mut intervals = Vec::new();
            assert!(!chunks.is_empty());
            assert!(chunks.is_sorted_by_key(|c| c.addr()));
            let mut ival_data = if ival.data.is_owned() {
                ival.data.take_data().unwrap()
            } else {
                assert!(ival.data.is_ref());
                let data = ival
                    .data
                    .get_data(&deduper.read().unwrap())
                    .unwrap()
                    .as_ref()
                    .to_vec();
                data
            };

            let ranges = {
                let mut ranges = Vec::with_capacity(chunks.len() + 2);
                let mut curr_addr = ival.start;
                for chunk in chunks {
                    if curr_addr < chunk.vaddr {
                        ranges.push((curr_addr, chunk.vaddr));
                        curr_addr = chunk.vaddr;
                    }

                    // check chunk is in range
                    assert!(
                        chunk.end() <= ival.end,
                        "invalid ord chunk for interval in range [{:#x?}; {:#x?}): {:x?}",
                        ival.start,
                        ival.end,
                        chunk
                    );

                    // might seem non-sensical, but if we have duplicate ord chunks, curr_addr
                    // might be == to chunk.end()
                    //
                    // we can't push empty ranges
                    if curr_addr < chunk.end() {
                        ranges.push((curr_addr, chunk.end()));
                        curr_addr = chunk.end();
                    }
                }
                if curr_addr < ival.end {
                    ranges.push((curr_addr, ival.end));
                }
                ranges
            };

            for (start, end) in ranges.into_iter().rev() {
                let off_start = start - ival.start;
                let off_end = end - ival.start;
                assert_eq!(off_end as usize, ival_data.len(), "we should be splitting off from the end of a vec with len {:x?}; splitting off from {off_end:x?}", ival_data.len());
                let data = ival_data.split_off(off_start as usize);
                ival_data.shrink_to_fit();
                let token = deduper.write().unwrap().insert(data);

                intervals.push(Interval {
                    start,
                    end,
                    data: Data::from_dedup_token(token),
                });
            }

            assert!(
                ival_data.is_empty(),
                "should've consumed the entire original vector, left with {:x?}B",
                ival_data.len()
            );
            intervals
        }

        let virt_range = self.virtual_range();

        let old_intervals = self
            .nodes
            .split_off(0)
            .into_iter()
            .flat_map(|node| node.ranges.into_iter())
            .filter(|x| !x.is_none())
            .collect::<Vec<_>>();

        // uninteresting pheader, skip
        if old_intervals.is_empty() {
            return Ok(());
        }

        let intervals = old_intervals
            .into_par_iter()
            .map(|mut ival| {
                let pertinent_chunks = {
                    let mut v = ord_chunks
                        .iter()
                        .filter(|x| x.kind == DataSource::Private)
                        .filter(|x| ival.start <= x.vaddr && x.vaddr < ival.end)
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
                        .copied()
                        .collect::<Vec<_>>();
                    v.sort_by_key(|x| x.addr());
                    assert!(v.iter().all(|x| x.n_pages > 0));
                    v.dedup_by_key(|x| x.addr());

                    v
                };

                if pertinent_chunks.is_empty() {
                    let data = if let Some(data) = ival.data.take_data() {
                        let token = deduper.write().unwrap().insert(data);
                        Data::from_dedup_token(token)
                    } else if let Some(token) = ival.data.dedup_token() {
                        Data::from_dedup_token(token)
                    } else {
                        assert!(!ival.data.is_data());
                        // not data, not reference, cloning is cheap
                        ival.data.clone()
                    };
                    vec![Interval {
                        start: ival.start,
                        end: ival.end,
                        data,
                    }]
                } else {
                    fracture_interval(ival, &pertinent_chunks, deduper.clone())
                }
            })
            .reduce(Vec::new, |mut accum, mut v| {
                accum.append(&mut v);
                accum
            });

        // VALIDATION phase

        // at this point, we should check that all the intervals are referenced data
        // (all of it was deduped)
        assert!(intervals
            .iter()
            .filter(|x| x.is_data())
            .all(|x| { x.data.is_ref() }));
        // the data referenced is of the correct length
        {
            let d = deduper.read().unwrap();
            assert!(intervals.iter().filter(|x| x.is_data()).all(|x| {
                x.data.get_data(&d).unwrap().as_ref().len() == (x.end - x.start) as usize
            }));
        }
        // no empty intervals
        {
            let d = deduper.read().unwrap();
            assert!(intervals
                .iter()
                .filter(|x| x.is_data())
                .all(|x| { x.data.get_data(&d).unwrap().as_ref().len() > 0 }));
        }

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

        // rebuild the ITREE

        *self = Self::build(intervals, self.virtual_range()).map_err(|error| {
            JifError::InvalidITree {
                virtual_range: self.virtual_range(),
                error,
            }
        })?;

        Ok(())
    }

    /// Bring owned data into the deduper
    pub fn dedup(&mut self, deduper: &mut RwLockWriteGuard<'_, Deduper>) {
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
        deduper: &'a RwLockReadGuard<'a, Deduper>,
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

    pub(crate) fn gen_empty<Data: IntervalData + Default + std::fmt::Debug + Send>() -> ITree<Data>
    {
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
        let deduper = RwLock::new(Deduper::default());
        assert_eq!(tree.iter_private_pages(&deduper.read().unwrap()).count(), 0);
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
