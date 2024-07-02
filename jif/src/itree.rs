use crate::error::*;
use crate::utils::{compare_pages, is_page_aligned, is_zero, PageCmp};

pub(crate) const FANOUT: usize = 4;
pub(crate) const IVAL_PER_NODE: usize = FANOUT - 1;

pub struct ITree {
    pub(crate) nodes: Vec<ITreeNode>,
}

#[derive(Default, Clone)]
pub struct ITreeNode {
    ranges: [Interval; IVAL_PER_NODE],
}

impl ITreeNode {
    pub(crate) const fn serialized_size() -> usize {
        IVAL_PER_NODE * Interval::serialized_size()
    }

    pub(crate) fn new(ranges: [Interval; IVAL_PER_NODE]) -> Self {
        ITreeNode { ranges }
    }

    pub(crate) fn ranges(&self) -> &[Interval] {
        &self.ranges
    }
}

#[derive(Clone, Copy)]
pub struct Interval {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) offset: u64,
}

#[derive(Clone, Copy)]
enum DiffState {
    Initial,
    AccumulatingData,
    AccumulatingZero,
}

pub fn create_itree_from_diff(
    base: &[u8],
    overlay: &mut Vec<u8>,
    virtual_base: u64,
) -> JifResult<ITree> {
    if !is_page_aligned(overlay.len() as u64) {
        return Err(JifError::ITreeError(ITreeError::OverlayAlignment(
            overlay.len(),
        )));
    } else if !is_page_aligned(base.len() as u64) {
        return Err(JifError::ITreeError(ITreeError::BaseAlignment(
            overlay.len(),
        )));
    }

    let mut data_offset = 0;
    let mut virtual_offset = 0;
    let mut intervals = Vec::new();
    let mut interval = Interval::new(0, 0, 0);
    let mut state = DiffState::Initial;
    for (base_page, overlay_page) in base.chunks_exact(0x1000).zip(overlay.chunks_exact(0x1000)) {
        state = match (state, compare_pages(base_page, overlay_page)) {
            (DiffState::Initial, PageCmp::Diff) => state,
            (DiffState::Initial, PageCmp::Same) => {
                interval.start = virtual_base + virtual_offset;
                interval.offset = data_offset;
                data_offset += overlay_page.len() as u64;
                DiffState::AccumulatingData
            }
            (DiffState::Initial, PageCmp::Zero) => {
                interval.start = virtual_base + virtual_offset;
                interval.offset = u64::MAX;
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingData, PageCmp::Diff) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(0, 0, 0);
                DiffState::Initial
            }
            (DiffState::AccumulatingData, PageCmp::Same) => {
                data_offset += overlay_page.len() as u64;
                state
            }
            (DiffState::AccumulatingData, PageCmp::Zero) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(virtual_base + virtual_offset, 0, u64::MAX);
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingZero, PageCmp::Diff) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(0, 0, 0);
                DiffState::Initial
            }
            (DiffState::AccumulatingZero, PageCmp::Same) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(virtual_base + virtual_offset, 0, data_offset);
                data_offset += overlay_page.len() as u64;
                state
            }
            (DiffState::AccumulatingZero, PageCmp::Zero) => state,
        };

        virtual_offset += base_page.len() as u64;
    }

    assert!(overlay.len() > base.len()); // TODO: figure out what happens if overlay.len() > base.len()

    if let Some(Interval { end, .. }) = intervals.last() {
        overlay.drain((*end as usize - virtual_base as usize)..);
    }
    for (begin, end) in intervals
        .iter()
        .zip(intervals.iter().skip(1))
        .map(|(i1, i2)| (i1.end - virtual_base, i2.start - virtual_base))
        .map(|(b, e)| (b as usize, e as usize))
        .rev()
    {
        overlay.drain(begin..end);
    }

    Ok(ITree::build(intervals))
}

pub fn create_itree_from_zero_page(data: &mut Vec<u8>, virtual_base: u64) -> JifResult<ITree> {
    if !is_page_aligned(data.len() as u64) {
        return Err(JifError::ITreeError(ITreeError::OverlayAlignment(
            data.len(),
        )));
    }

    let mut data_offset = 0;
    let mut virtual_offset = 0;
    let mut intervals = Vec::new();
    let mut interval = Interval::new(0, 0, 0);
    let mut state = DiffState::Initial;
    for page in data.chunks_exact(0x1000) {
        state = match (state, is_zero(page)) {
            (DiffState::Initial, false) => {
                interval.start = virtual_base + virtual_offset;
                interval.offset = data_offset;
                data_offset += page.len() as u64;
                DiffState::AccumulatingData
            }
            (DiffState::Initial, true) => {
                interval.start = virtual_base + virtual_offset;
                interval.offset = u64::MAX;
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingData, false) => {
                data_offset += page.len() as u64;
                state
            }
            (DiffState::AccumulatingData, true) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(virtual_base + virtual_offset, 0, u64::MAX);
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingZero, false) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(virtual_base + virtual_offset, 0, data_offset);
                data_offset += page.len() as u64;
                state
            }
            (DiffState::AccumulatingZero, true) => state,
        };

        virtual_offset += page.len() as u64;
    }

    if let Some(Interval { end, .. }) = intervals.last() {
        data.drain((*end as usize - virtual_base as usize)..);
    }
    for (begin, end) in intervals
        .iter()
        .zip(intervals.iter().skip(1))
        .map(|(i1, i2)| (i1.end - virtual_base, i2.start - virtual_base))
        .map(|(b, e)| (b as usize, e as usize))
        .rev()
    {
        data.drain(begin..end);
    }

    Ok(ITree::build(intervals))
}

impl ITree {
    pub fn new(nodes: Vec<ITreeNode>) -> Self {
        ITree { nodes }
    }

    const fn n_itree_nodes_from_intervals(n_intervals: usize) -> usize {
        (n_intervals + FANOUT - 2) / (FANOUT - 1)
    }

    pub fn build(mut intervals: Vec<Interval>) -> Self {
        fn fill(
            nodes: &mut Vec<ITreeNode>,
            intervals: &[Interval],
            interval_cursor: &mut usize,
            node_idx: usize,
        ) {
            // first base case: no node with this index
            if node_idx >= nodes.len() {
                return;
            }

            let mut child_idx = node_idx * FANOUT + 1;
            for i in 0..(FANOUT - 1) {
                // recursion
                fill(nodes, intervals, interval_cursor, child_idx);

                // second base case: no more intervals
                if *interval_cursor >= intervals.len() {
                    return;
                }

                // insert an interval
                nodes[node_idx].ranges[i] = intervals[*interval_cursor];
                *interval_cursor += 1;
                child_idx += 1;
            }

            // FANOUT == IVAL_PER_NODE - 1, so we need to insert right_most child
            fill(nodes, intervals, interval_cursor, child_idx);
        }

        let n_nodes = Self::n_itree_nodes_from_intervals(intervals.len());
        let mut nodes = (0..n_nodes)
            .into_iter()
            .map(|_| ITreeNode::default())
            .collect::<Vec<_>>();

        intervals.sort_by_key(|it| it.start);
        let mut interval_cursor = 0;
        fill(&mut nodes, &intervals, &mut interval_cursor, 0);
        ITree::new(nodes)
    }

    pub fn n_nodes(&self) -> usize {
        self.nodes.len()
    }
}

impl Interval {
    pub(crate) const fn serialized_size() -> usize {
        3 * std::mem::size_of::<u64>()
    }

    pub(crate) fn new(start: u64, end: u64, offset: u64) -> Self {
        Interval { start, end, offset }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.start == u64::MAX || self.end == u64::MAX || self.offset == u64::MAX
    }
}

impl Default for Interval {
    fn default() -> Self {
        Interval {
            start: u64::MAX,
            end: u64::MAX,
            offset: u64::MAX,
        }
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
            .entries(self.ranges.iter().filter(|i| !i.is_empty()))
            .finish()
    }
}

impl std::fmt::Debug for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            f.debug_struct("EmptyInterval").finish()
        } else {
            f.write_fmt(format_args!(
                "[{:#x}; {:#x}) -> {:#x}",
                &self.start, &self.end, &self.offset
            ))
        }
    }
}
