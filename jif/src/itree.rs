use crate::utils::{compare_pages, is_page_aligned, is_zero, PageCmp, PAGE_SIZE};

pub(crate) const FANOUT: usize = 4;
pub(crate) const IVAL_PER_NODE: usize = FANOUT - 1;

pub struct ITree {
    pub(crate) nodes: Vec<ITreeNode>,
}

#[derive(Default, Clone, PartialEq, Eq)]
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

    pub(crate) fn shift_offsets(&mut self, new_base: i64) {
        for interval in self.ranges.iter_mut() {
            interval.shift_offset(new_base);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) offset: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffState {
    Initial,
    AccumulatingData,
    AccumulatingZero,
}

pub fn create_itree_from_diff(base: &[u8], overlay: &mut Vec<u8>, virtual_base: u64) -> ITree {
    fn remove_gaps(overlay: &mut Vec<u8>, virtual_base: u64, intervals: &[Interval]) {
        // remove trailing empty space
        if let Some(Interval { start, end, offset }) = intervals.last() {
            overlay.drain(((*end - virtual_base) as usize)..);

            if *offset == u64::MAX {
                overlay.drain(((*start - virtual_base) as usize)..((*end - virtual_base) as usize));
            }
        } else {
            // no intervals means it is a pure segment
            overlay.clear();
            return;
        }

        // create drain ranges for
        //  1. gaps between intervals
        //  2. zero sections
        let mut drain_ranges = Vec::new();
        intervals
            .iter()
            .zip(intervals.iter().skip(1))
            .for_each(|(i1, i2)| {
                let gap = (
                    (i1.end - virtual_base) as usize,
                    (i2.start - virtual_base) as usize,
                );
                drain_ranges.push(gap);

                if i1.offset == u64::MAX {
                    drain_ranges.push((
                        (i1.start - virtual_base) as usize,
                        (i1.end - virtual_base) as usize,
                    ));
                }
            });

        // sort drain ranges by *descending* order of end address
        drain_ranges.sort_by(|(_a_start, a_end), (_b_start, b_end)| b_end.cmp(a_end));

        for (start, end) in drain_ranges {
            overlay.drain(start..end);
        }

        // remove leading empty space
        if let Some(Interval { start, .. }) = intervals.first() {
            overlay.drain(..((*start - virtual_base) as usize));
        }
    }

    assert!(
        is_page_aligned(overlay.len() as u64),
        "the overlay should be page aligned because the data segment should be page aligned"
    );
    assert!(
        is_page_aligned(base.len() as u64),
        "the base should be page aligned because we extend it"
    );

    let mut data_offset = 0;
    let mut virtual_offset = 0;
    let mut intervals = Vec::new();
    let mut interval = Interval::new(0, 0, 0);
    let mut state = DiffState::Initial;
    for (base_page, overlay_page) in base
        .chunks_exact(PAGE_SIZE)
        .zip(overlay.chunks_exact(PAGE_SIZE))
    {
        state = match (state, compare_pages(base_page, overlay_page)) {
            (DiffState::Initial, PageCmp::Same) => state,
            (DiffState::Initial, PageCmp::Diff) => {
                interval.start = virtual_base + virtual_offset;
                interval.offset = data_offset;
                data_offset += PAGE_SIZE as u64;
                DiffState::AccumulatingData
            }
            (DiffState::Initial, PageCmp::Zero) => {
                interval.start = virtual_base + virtual_offset;
                interval.offset = u64::MAX;
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingData, PageCmp::Same) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(0, 0, 0);
                DiffState::Initial
            }
            (DiffState::AccumulatingData, PageCmp::Diff) => {
                data_offset += PAGE_SIZE as u64;
                state
            }
            (DiffState::AccumulatingData, PageCmp::Zero) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(virtual_base + virtual_offset, 0, u64::MAX);
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingZero, PageCmp::Same) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(0, 0, 0);
                DiffState::Initial
            }
            (DiffState::AccumulatingZero, PageCmp::Diff) => {
                interval.end = virtual_base + virtual_offset;
                intervals.push(interval);
                interval = Interval::new(virtual_base + virtual_offset, 0, data_offset);
                data_offset += PAGE_SIZE as u64;
                state
            }
            (DiffState::AccumulatingZero, PageCmp::Zero) => state,
        };

        virtual_offset += PAGE_SIZE as u64;
    }

    if overlay.len() > base.len() {
        for page in overlay
            .chunks_exact(PAGE_SIZE)
            .skip(virtual_offset as usize / PAGE_SIZE)
        {
            state = match (state, is_zero(page)) {
                (DiffState::Initial, false) => {
                    interval.start = virtual_base + virtual_offset;
                    interval.offset = data_offset;
                    data_offset += PAGE_SIZE as u64;
                    DiffState::AccumulatingData
                }
                (DiffState::Initial, true) => {
                    interval.start = virtual_base + virtual_offset;
                    interval.offset = u64::MAX;
                    DiffState::AccumulatingZero
                }
                (DiffState::AccumulatingData, false) => {
                    data_offset += PAGE_SIZE as u64;
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
                    data_offset += PAGE_SIZE as u64;
                    DiffState::AccumulatingData
                }
                (DiffState::AccumulatingZero, true) => state,
            };

            virtual_offset += PAGE_SIZE as u64;
        }
    }

    // last interval
    if state != DiffState::Initial {
        interval.end = virtual_base + virtual_offset;
        intervals.push(interval);
    }

    remove_gaps(overlay, virtual_base, &intervals);
    ITree::build(intervals)
}

pub fn create_itree_from_zero_page(data: &mut Vec<u8>, virtual_base: u64) -> ITree {
    fn remove_gaps(data: &mut Vec<u8>, virtual_base: u64, intervals: &[Interval]) {
        intervals
            .iter()
            .rev()
            .filter(|i| i.offset == u64::MAX)
            .for_each(|i| {
                let start = (i.start - virtual_base) as usize;
                let end = (i.end - virtual_base) as usize;
                data.drain(start..end);
            });
    }
    assert!(
        is_page_aligned(data.len() as u64),
        "data should be page aligned because data segments are page aligned"
    );
    let mut data_offset = 0;
    let mut virtual_offset = 0;
    let mut intervals = Vec::new();
    let mut interval = Interval::new(0, 0, 0);
    let mut state = DiffState::Initial;
    for page in data.chunks_exact(PAGE_SIZE) {
        state = match (state, is_zero(page)) {
            (DiffState::Initial, false) => {
                interval.start = virtual_base + virtual_offset;
                interval.offset = data_offset;
                data_offset += PAGE_SIZE as u64;
                DiffState::AccumulatingData
            }
            (DiffState::Initial, true) => {
                interval.start = virtual_base + virtual_offset;
                interval.offset = u64::MAX;
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingData, false) => {
                data_offset += PAGE_SIZE as u64;
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
                data_offset += PAGE_SIZE as u64;
                DiffState::AccumulatingData
            }
            (DiffState::AccumulatingZero, true) => state,
        };

        virtual_offset += PAGE_SIZE as u64;
    }

    // last interval
    if state != DiffState::Initial {
        interval.end = virtual_base + virtual_offset;
        intervals.push(interval);
    }

    remove_gaps(data, virtual_base, &intervals);
    ITree::build(intervals)
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

    pub fn shift_offsets(&mut self, new_base: i64) {
        for n in self.nodes.iter_mut() {
            n.shift_offsets(new_base)
        }
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
        self.start == u64::MAX || self.end == u64::MAX
    }

    pub(crate) fn shift_offset(&mut self, new_base: i64) {
        if !self.is_empty() && self.offset != u64::MAX {
            if new_base > 0 {
                self.offset += new_base as u64
            } else {
                self.offset -= new_base.abs() as u64
            }
        }
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    // test that it can create an interval tree no zero pages
    fn create_zero_0() {
        let mut data = vec![0xff; 0x1000 * 5];

        let itree = create_itree_from_zero_page(&mut data, 0x0000).unwrap();
        let target_itree = ITree::build(vec![Interval::new(0x0000, 0x5000, 0x0000)]);

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(data.len(), 0x1000 * 5);
    }

    #[test]
    // test that it can create an interval tree all zero pages
    fn create_zero_1() {
        let mut data = vec![0x00; 0x1000 * 5];

        let itree = create_itree_from_zero_page(&mut data, 0x0000).unwrap();
        let target_itree = ITree::build(vec![Interval::new(0x0000, 0x5000, u64::MAX)]);

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(data.len(), 0x1000 * 0);
    }

    #[test]
    // test that it can create an interval tree with a trailing zero range
    fn create_zero_2() {
        let mut data = vec![0x00u8; 0x1000 * 5];
        data[0x0000] = 0xff;
        data[0x2000] = 0xff;

        let itree = create_itree_from_zero_page(&mut data, 0x0000).unwrap();
        let target_itree = ITree::build(vec![
            Interval::new(0x0000, 0x1000, 0x0000),
            Interval::new(0x1000, 0x2000, u64::MAX),
            Interval::new(0x2000, 0x3000, 0x1000),
            Interval::new(0x3000, 0x5000, u64::MAX),
        ]);

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(data.len(), 0x1000 * 2);
    }

    #[test]
    // test that it can create an interval tree with a trailing data range
    fn create_zero_3() {
        let mut data = vec![0x00u8; 0x1000 * 5];
        data[0x0000] = 0xff;
        data[0x3000] = 0xff;
        data[0x4000] = 0xff;

        let itree = create_itree_from_zero_page(&mut data, 0x0000).unwrap();
        let target_itree = ITree::build(vec![
            Interval::new(0x0000, 0x1000, 0x0000),
            Interval::new(0x1000, 0x3000, u64::MAX),
            Interval::new(0x3000, 0x5000, 0x1000),
        ]);

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(data.len(), 0x1000 * 3);
    }

    #[test]
    // test that it can create an interval tree when there is no difference
    fn create_diff_0() {
        let base = vec![0xffu8; 0x1000 * 5];
        let mut overlay = vec![0xffu8; 0x1000 * 5];

        let itree = create_itree_from_diff(&base, &mut overlay, 0x0000).unwrap();
        let target_itree = ITree::build(vec![]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(overlay.len(), 0x1000 * 0);
    }

    #[test]
    // test that it can create an interval tree when there is no similarity
    fn create_diff_1() {
        let base = vec![0xffu8; 0x1000 * 5];
        let mut overlay = vec![0x88u8; 0x1000 * 5];

        let itree = create_itree_from_diff(&base, &mut overlay, 0x0000).unwrap();
        let target_itree = ITree::build(vec![Interval::new(0x0000, 0x5000, 0x0000)]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(overlay.len(), 0x1000 * 5);
    }

    #[test]
    // test that it can create an interval tree when the overlay is zero
    fn create_diff_2() {
        let base = vec![0xffu8; 0x1000 * 5];
        let mut overlay = vec![0x00u8; 0x1000 * 5];

        let itree = create_itree_from_diff(&base, &mut overlay, 0x0000).unwrap();
        let target_itree = ITree::build(vec![Interval::new(0x0000, 0x5000, u64::MAX)]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(overlay.len(), 0x1000 * 0);
    }

    #[test]
    // test that it can create an interval tree when the overlay is bigger than the base
    // include the fact that the overlay over-region may have zero pages
    fn create_diff_3() {
        let base = vec![0xffu8; 0x1000 * 1];
        let mut overlay = vec![0xffu8; 0x1000 * 5];
        overlay[0x4000..].fill(0x00);

        let itree = create_itree_from_diff(&base, &mut overlay, 0x0000).unwrap();
        let target_itree = ITree::build(vec![
            Interval::new(0x1000, 0x4000, 0x0000),
            Interval::new(0x4000, 0x5000, u64::MAX),
        ]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(overlay.len(), 0x1000 * 3);
    }

    #[test]
    // complete test:
    //  - include overlay over-extension with trailing zeroes
    //  - include sparse pages
    fn create_diff_4() {
        let base = vec![0xffu8; 0x1000 * 6];
        let mut overlay = vec![0x00u8; 0x1000 * 10];
        overlay[0x0000..0x1000].fill(0xff); // same
        overlay[0x1000..0x2000].fill(0x00); // zero
        overlay[0x2000..0x3000].fill(0x00); // zero
        overlay[0x3000..0x4000].fill(0xaa); // diff
        overlay[0x4000..0x5000].fill(0xff); // same
        overlay[0x5000..0x6000].fill(0xaa); // diff

        overlay[0x6000..0x7000].fill(0xff); // non-zero
        overlay[0x7000..0x8000].fill(0x00); // zero
        overlay[0x8000..0x9000].fill(0xff); // non-zero
        overlay[0x9000..0xa000].fill(0x00); // zero

        let itree = create_itree_from_diff(&base, &mut overlay, 0x0000).unwrap();
        let target_itree = ITree::build(vec![
            Interval::new(0x1000, 0x3000, u64::MAX),
            Interval::new(0x3000, 0x4000, 0x0000),
            Interval::new(0x5000, 0x7000, 0x1000),
            Interval::new(0x7000, 0x8000, u64::MAX),
            Interval::new(0x8000, 0x9000, 0x3000),
            Interval::new(0x9000, 0xa000, u64::MAX),
        ]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(overlay.len(), 0x1000 * 4);
    }
}
