use crate::itree_node::{ITreeNode, Interval, IntervalData, RawInterval, FANOUT};
use crate::utils::{compare_pages, is_page_aligned, is_zero, PageCmp, PAGE_SIZE};

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
    pub fn new(nodes: Vec<ITreeNode>) -> Self {
        ITree { nodes }
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
    pub fn build(mut intervals: Vec<Interval>) -> Self {
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
        ITree::new(nodes)
    }

    /// Size of the interval tree in number of nodes
    pub fn n_nodes(&self) -> usize {
        self.nodes.len()
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
    pub fn mapped_subregion_size(&self, start: u64, end: u64) -> usize {
        self.nodes
            .iter()
            .map(|n| n.mapped_subregion_size(start, end))
            .sum()
    }

    /// How much of a particular `[start; end)` sub-interval of the address space
    /// does this interval tree not map (i.e., will be backed by a reference segment)
    pub fn not_mapped_subregion_size(&self, start: u64, end: u64) -> usize {
        (end - start) as usize - self.mapped_subregion_size(start, end)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffState {
    Initial,
    AccumulatingData,
    AccumulatingZero,
}

/// Create an interval tree by diffing a base (reference file) with an overlay (saved data)
pub(crate) fn create_itree_from_diff(base: &[u8], overlay: Vec<u8>, virtual_base: u64) -> ITree {
    assert!(
        is_page_aligned(overlay.len() as u64),
        "the overlay should be page aligned because the data segment should be page aligned"
    );
    assert!(
        is_page_aligned(base.len() as u64),
        "the base should be page aligned because we extend it"
    );

    let mut offset = 0;
    let mut raw_intervals = Vec::new();
    let mut interval = RawInterval::default();
    let mut state = DiffState::Initial;
    for (base_page, overlay_page) in base
        .chunks_exact(PAGE_SIZE)
        .zip(overlay.chunks_exact(PAGE_SIZE))
    {
        let virtual_offset = virtual_base + offset;
        state = match (state, compare_pages(base_page, overlay_page)) {
            (DiffState::Initial, PageCmp::Same) => state,
            (DiffState::Initial, PageCmp::Diff) => {
                interval.start = virtual_offset;
                interval.offset = offset;
                DiffState::AccumulatingData
            }
            (DiffState::Initial, PageCmp::Zero) => {
                interval.start = virtual_offset;
                interval.offset = u64::MAX;
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingData, PageCmp::Same) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::default();
                DiffState::Initial
            }
            (DiffState::AccumulatingData, PageCmp::Diff) => state,
            (DiffState::AccumulatingData, PageCmp::Zero) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::new(virtual_offset, 0, u64::MAX);
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingZero, PageCmp::Same) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::default();
                DiffState::Initial
            }
            (DiffState::AccumulatingZero, PageCmp::Diff) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::new(virtual_offset, 0, offset);
                DiffState::AccumulatingData
            }
            (DiffState::AccumulatingZero, PageCmp::Zero) => state,
        };

        offset += PAGE_SIZE as u64;
    }

    if overlay.len() > base.len() {
        let virtual_offset = virtual_base + offset;
        for page in overlay
            .chunks_exact(PAGE_SIZE)
            .skip(virtual_offset as usize / PAGE_SIZE)
        {
            let virtual_offset = virtual_base + offset;
            state = match (state, is_zero(page)) {
                (DiffState::Initial, false) => {
                    interval.start = virtual_offset;
                    interval.offset = offset;
                    DiffState::AccumulatingData
                }
                (DiffState::Initial, true) => {
                    interval.start = virtual_offset;
                    interval.offset = u64::MAX;
                    DiffState::AccumulatingZero
                }
                (DiffState::AccumulatingData, false) => state,
                (DiffState::AccumulatingData, true) => {
                    interval.end = virtual_offset;
                    raw_intervals.push(interval);

                    interval = RawInterval::new(virtual_offset, 0, u64::MAX);
                    DiffState::AccumulatingZero
                }
                (DiffState::AccumulatingZero, false) => {
                    interval.end = virtual_offset;
                    raw_intervals.push(interval);

                    interval = RawInterval::new(virtual_offset, 0, offset);
                    DiffState::AccumulatingData
                }
                (DiffState::AccumulatingZero, true) => state,
            };

            offset += PAGE_SIZE as u64;
        }
    }

    // last interval
    if state != DiffState::Initial {
        let virtual_offset = virtual_base + offset;
        interval.end = virtual_offset;
        raw_intervals.push(interval);
    }

    ITree::build(materialize_raw_intervals(raw_intervals, overlay))
}

/// Create an interval tree from a privately mapped region (by removing zero pages)
pub(crate) fn create_itree_from_zero_page(data: Vec<u8>, virtual_base: u64) -> ITree {
    assert!(
        is_page_aligned(data.len() as u64),
        "data should be page aligned because data segments are page aligned"
    );

    let mut offset = 0;
    let mut raw_intervals = Vec::new();
    let mut interval = RawInterval::default();
    let mut state = DiffState::Initial;
    for page in data.chunks_exact(PAGE_SIZE) {
        let virtual_offset = virtual_base + offset;
        state = match (state, is_zero(page)) {
            (DiffState::Initial, false) => {
                interval.start = virtual_offset;
                interval.offset = offset;
                DiffState::AccumulatingData
            }
            (DiffState::Initial, true) => {
                interval.start = virtual_offset;
                interval.offset = u64::MAX;
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingData, false) => state,
            (DiffState::AccumulatingData, true) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::new(virtual_offset, 0, u64::MAX);
                DiffState::AccumulatingZero
            }
            (DiffState::AccumulatingZero, false) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);
                interval = RawInterval::new(virtual_offset, 0, offset);
                DiffState::AccumulatingData
            }
            (DiffState::AccumulatingZero, true) => state,
        };

        offset += PAGE_SIZE as u64;
    }

    // last interval
    if state != DiffState::Initial {
        interval.end = virtual_base + offset;
        raw_intervals.push(interval);
    }

    ITree::build(materialize_raw_intervals(raw_intervals, data))
}

/// Materialize the raw intervals by stealing data from the data
fn materialize_raw_intervals(raw_intervals: Vec<RawInterval>, mut data: Vec<u8>) -> Vec<Interval> {
    // note: the raw intervals are sorted by ascending order of offset
    raw_intervals
        .into_iter()
        .rev()
        .map(|raw| {
            if raw.is_empty() {
                Interval::default()
            } else if raw.is_zero() {
                Interval::new(raw.start, raw.end, IntervalData::Zero)
            } else {
                let len = raw.len();
                let mut ival_data = data.split_off(raw.offset as usize);
                let _ = ival_data.split_off(len as usize); // discard extra data (may have been shared)
                Interval::new(raw.start, raw.end, IntervalData::Data(ival_data))
            }
        })
        .collect()
}

impl std::fmt::Debug for ITree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.nodes.iter()).finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    // test that it can create an interval tree no zero pages
    fn create_zero_0() {
        let data = vec![0xff; 0x1000 * 5];

        let itree = create_itree_from_zero_page(data, 0x0000);
        let target_itree = ITree::build(vec![Interval::new(
            0x0000,
            0x5000,
            IntervalData::Data(vec![0xff; 0x1000 * 5]),
        )]);

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0);
        assert_eq!(itree.private_data_size(), 0x1000 * 5);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0x3000), 0x1000 * 3);
    }

    #[test]
    // test that it can create an interval tree all zero pages
    fn create_zero_1() {
        let data = vec![0x00; 0x1000 * 5];

        let itree = create_itree_from_zero_page(data, 0x0000);
        let target_itree = ITree::build(vec![Interval::new(0x0000, 0x5000, IntervalData::Zero)]);

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 5);
        assert_eq!(itree.private_data_size(), 0);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0x3000), 0x1000 * 3);
    }

    #[test]
    // test that it can create an interval tree with a trailing zero range
    fn create_zero_2() {
        let mut data = vec![0x00u8; 0x1000 * 5];
        data[0x0000..0x1000].fill(0xff);
        data[0x2000..0x3000].fill(0xff);

        let itree = create_itree_from_zero_page(data, 0x0000);
        let target_itree = ITree::build(vec![
            Interval::new(0x0000, 0x1000, IntervalData::Data(vec![0xff; 0x1000])),
            Interval::new(0x1000, 0x2000, IntervalData::Zero),
            Interval::new(0x2000, 0x3000, IntervalData::Data(vec![0xff; 0x1000])),
            Interval::new(0x3000, 0x5000, IntervalData::Zero),
        ]);

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 3);
        assert_eq!(itree.private_data_size(), 0x1000 * 2);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0x4000), 0x1000 * 4);
    }

    #[test]
    // test that it can create an interval tree with a trailing data range
    fn create_zero_3() {
        let mut data = vec![0x00u8; 0x1000 * 5];
        data[0x0000..0x1000].fill(0xff);
        data[0x3000..0x5000].fill(0xff);

        let itree = create_itree_from_zero_page(data, 0x0000);
        let target_itree = ITree::build(vec![
            Interval::new(0x0000, 0x1000, IntervalData::Data(vec![0xff; 0x1000])),
            Interval::new(0x1000, 0x3000, IntervalData::Zero),
            Interval::new(0x3000, 0x5000, IntervalData::Data(vec![0xff; 0x2000])),
        ]);

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 2);
        assert_eq!(itree.private_data_size(), 0x1000 * 3);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0x4000), 0x1000 * 4);
    }

    #[test]
    // test that it can create an interval tree when there is no difference
    fn create_diff_0() {
        let base = vec![0xffu8; 0x1000 * 5];
        let overlay = vec![0xffu8; 0x1000 * 5];

        let itree = create_itree_from_diff(&base, overlay, 0x0000);
        let target_itree = ITree::build(vec![]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 0);
        assert_eq!(itree.private_data_size(), 0x1000 * 0);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0x5000), 0x1000 * 0);
        assert_eq!(itree.not_mapped_subregion_size(0x0000, 0x5000), 0x1000 * 5);
    }

    #[test]
    // test that it can create an interval tree when there is no similarity
    fn create_diff_1() {
        let base = vec![0xffu8; 0x1000 * 5];
        let overlay = vec![0x88u8; 0x1000 * 5];

        let itree = create_itree_from_diff(&base, overlay, 0x0000);
        let target_itree = ITree::build(vec![Interval::new(
            0x0000,
            0x5000,
            IntervalData::Data(vec![0x88u8; 0x1000 * 5]),
        )]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 0);
        assert_eq!(itree.private_data_size(), 0x1000 * 5);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0x5000), 0x1000 * 5);
        assert_eq!(itree.not_mapped_subregion_size(0x0000, 0x5000), 0x1000 * 0);
    }

    #[test]
    // test that it can create an interval tree when the overlay is zero
    fn create_diff_2() {
        let base = vec![0xffu8; 0x1000 * 5];
        let overlay = vec![0x00u8; 0x1000 * 5];

        let itree = create_itree_from_diff(&base, overlay, 0x0000);
        let target_itree = ITree::build(vec![Interval::new(0x0000, 0x5000, IntervalData::Zero)]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 5);
        assert_eq!(itree.private_data_size(), 0x1000 * 0);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0x5000), 0x1000 * 5);
        assert_eq!(itree.not_mapped_subregion_size(0x0000, 0x5000), 0x1000 * 0);
    }

    #[test]
    // test that it can create an interval tree when the overlay is bigger than the base
    // include the fact that the overlay over-region may have zero pages
    fn create_diff_3() {
        let base = vec![0xffu8; 0x1000 * 1];
        let mut overlay = vec![0xffu8; 0x1000 * 5];
        overlay[0x4000..].fill(0x00);

        let itree = create_itree_from_diff(&base, overlay, 0x0000);
        let target_itree = ITree::build(vec![
            Interval::new(0x1000, 0x4000, IntervalData::Data(vec![0xffu8; 0x1000 * 3])),
            Interval::new(0x4000, 0x5000, IntervalData::Zero),
        ]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 1);
        assert_eq!(itree.private_data_size(), 0x1000 * 3);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0x5000), 0x1000 * 4);
        assert_eq!(itree.not_mapped_subregion_size(0x0000, 0x5000), 0x1000 * 1);
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

        let itree = create_itree_from_diff(&base, overlay, 0x0000);
        let target_itree = ITree::build(vec![
            Interval::new(0x1000, 0x3000, IntervalData::Zero),
            Interval::new(0x3000, 0x4000, IntervalData::Data(vec![0xaa; 0x1000 * 1])),
            Interval::new(
                0x5000,
                0x7000,
                IntervalData::Data({
                    let mut v = vec![0x00; 0x1000 * 2];
                    v[..0x1000].fill(0xaa);
                    v[0x1000..].fill(0xff);
                    v
                }),
            ),
            Interval::new(0x7000, 0x8000, IntervalData::Zero),
            Interval::new(0x8000, 0x9000, IntervalData::Data(vec![0xff; 0x1000 * 1])),
            Interval::new(0x9000, 0xa000, IntervalData::Zero),
        ]);
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 4);
        assert_eq!(itree.private_data_size(), 0x1000 * 4);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0x5000), 0x1000 * 3);
        assert_eq!(itree.not_mapped_subregion_size(0x0000, 0x5000), 0x1000 * 2);
        assert_eq!(itree.mapped_subregion_size(0x0000, 0xa000), 0x1000 * 8);
        assert_eq!(itree.not_mapped_subregion_size(0x0000, 0xa000), 0x1000 * 2);
    }
}
