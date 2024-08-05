//! Interval tree building logic
use crate::itree::interval::{AnonIntervalData, Interval, RawInterval, RefIntervalData};
use crate::utils::{compare_pages, is_page_aligned, is_zero, PageCmp, PAGE_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnonDiffState {
    Initial,
    AccumulatingData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefDiffState {
    Initial,
    AccumulatingData,
    AccumulatingZero,
}

/// Create an [`ITree`] from a privately mapped region (by removing zero pages)
pub(crate) fn create_anon_itree_from_zero_page(
    data: &[u8],
    virtual_base: u64,
    intervals: &mut Vec<Interval<AnonIntervalData>>,
) {
    assert!(
        is_page_aligned(data.len() as u64),
        "data should be page aligned because data segments are page aligned"
    );

    let mut offset = 0;
    let mut raw_intervals = Vec::new();
    let mut interval = RawInterval::default();
    let mut state = AnonDiffState::Initial;
    for page in data.chunks_exact(PAGE_SIZE) {
        let virtual_offset = virtual_base + offset;
        state = match (state, is_zero(page)) {
            (AnonDiffState::Initial, false) => {
                interval.start = virtual_offset;
                interval.offset = offset;
                AnonDiffState::AccumulatingData
            }
            (AnonDiffState::Initial, true) => state,

            (AnonDiffState::AccumulatingData, false) => state,
            (AnonDiffState::AccumulatingData, true) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::default();
                AnonDiffState::Initial
            }
        };

        offset += PAGE_SIZE as u64;
    }

    // last interval
    if state != AnonDiffState::Initial {
        interval.end = virtual_base + offset;
        raw_intervals.push(interval);
    }

    materialize_raw_anon_intervals(raw_intervals, data, intervals)
}

/// Create an [`ITree`] from a privately mapped region (by removing zero pages)
pub(crate) fn create_ref_itree_from_zero_page(
    data: &[u8],
    virtual_base: u64,
    intervals: &mut Vec<Interval<RefIntervalData>>,
) {
    assert!(
        is_page_aligned(data.len() as u64),
        "data should be page aligned because data segments are page aligned"
    );

    let mut offset = 0;
    let mut raw_intervals = Vec::new();
    let mut interval = RawInterval::default();
    let mut state = RefDiffState::Initial;
    for page in data.chunks_exact(PAGE_SIZE) {
        let virtual_offset = virtual_base + offset;
        state = match (state, is_zero(page)) {
            (RefDiffState::Initial, false) => {
                interval.start = virtual_offset;
                interval.offset = offset;
                RefDiffState::AccumulatingData
            }
            (RefDiffState::Initial, true) => {
                interval.start = virtual_offset;
                interval.offset = u64::MAX;
                RefDiffState::AccumulatingZero
            }
            (RefDiffState::AccumulatingZero, false) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::new(virtual_offset, 0, offset);
                RefDiffState::AccumulatingData
            }
            (RefDiffState::AccumulatingZero, true) => state,

            (RefDiffState::AccumulatingData, false) => state,
            (RefDiffState::AccumulatingData, true) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::new(virtual_offset, 0, u64::MAX);
                RefDiffState::AccumulatingZero
            }
        };

        offset += PAGE_SIZE as u64;
    }

    // last interval
    if state != RefDiffState::Initial {
        interval.end = virtual_base + offset;
        raw_intervals.push(interval);
    }

    materialize_raw_ref_intervals(raw_intervals, data, intervals)
}

/// Create an [`ITree`] by diffing a base (reference file) with an overlay (saved data)
pub(crate) fn create_itree_from_diff(
    base: &[u8],
    overlay: &[u8],
    virtual_base: u64,
    intervals: &mut Vec<Interval<RefIntervalData>>,
) {
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
    let mut state = RefDiffState::Initial;
    for (base_page, overlay_page) in base
        .chunks_exact(PAGE_SIZE)
        .zip(overlay.chunks_exact(PAGE_SIZE))
    {
        let virtual_offset = virtual_base + offset;
        state = match (state, compare_pages(base_page, overlay_page)) {
            (RefDiffState::Initial, PageCmp::Same) => state,
            (RefDiffState::Initial, PageCmp::Diff) => {
                interval.start = virtual_offset;
                interval.offset = offset;
                RefDiffState::AccumulatingData
            }
            (RefDiffState::Initial, PageCmp::Zero) => {
                interval.start = virtual_offset;
                interval.offset = u64::MAX;
                RefDiffState::AccumulatingZero
            }
            (RefDiffState::AccumulatingData, PageCmp::Same) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::default();
                RefDiffState::Initial
            }
            (RefDiffState::AccumulatingData, PageCmp::Diff) => state,
            (RefDiffState::AccumulatingData, PageCmp::Zero) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::new(virtual_offset, 0, u64::MAX);
                RefDiffState::AccumulatingZero
            }
            (RefDiffState::AccumulatingZero, PageCmp::Same) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::default();
                RefDiffState::Initial
            }
            (RefDiffState::AccumulatingZero, PageCmp::Diff) => {
                interval.end = virtual_offset;
                raw_intervals.push(interval);

                interval = RawInterval::new(virtual_offset, 0, offset);
                RefDiffState::AccumulatingData
            }
            (RefDiffState::AccumulatingZero, PageCmp::Zero) => state,
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
                (RefDiffState::Initial, false) => {
                    interval.start = virtual_offset;
                    interval.offset = offset;
                    RefDiffState::AccumulatingData
                }
                (RefDiffState::Initial, true) => {
                    interval.start = virtual_offset;
                    interval.offset = u64::MAX;
                    RefDiffState::AccumulatingZero
                }
                (RefDiffState::AccumulatingData, false) => state,
                (RefDiffState::AccumulatingData, true) => {
                    interval.end = virtual_offset;
                    raw_intervals.push(interval);

                    interval = RawInterval::new(virtual_offset, 0, u64::MAX);
                    RefDiffState::AccumulatingZero
                }
                (RefDiffState::AccumulatingZero, false) => {
                    interval.end = virtual_offset;
                    raw_intervals.push(interval);

                    interval = RawInterval::new(virtual_offset, 0, offset);
                    RefDiffState::AccumulatingData
                }
                (RefDiffState::AccumulatingZero, true) => state,
            };

            offset += PAGE_SIZE as u64;
        }
    }

    // last interval
    if state != RefDiffState::Initial {
        let virtual_offset = virtual_base + offset;
        interval.end = virtual_offset;
        raw_intervals.push(interval);
    }

    materialize_raw_ref_intervals(raw_intervals, overlay, intervals)
}

/// Materialize the [`RawInterval`] by stealing data from the data
fn materialize_raw_anon_intervals(
    raw_intervals: Vec<RawInterval>,
    data: &[u8],
    intervals: &mut Vec<Interval<AnonIntervalData>>,
) {
    intervals.reserve(raw_intervals.len());
    raw_intervals
        .into_iter()
        .map(|raw| {
            assert!(!raw.is_zero());
            if raw.is_empty() {
                Interval::default()
            } else {
                let len = raw.len();
                let data_begin = raw.offset as usize;
                let data_end = (raw.offset + len) as usize;
                let ival_data = data[data_begin..data_end].to_vec();
                Interval::new(raw.start, raw.end, AnonIntervalData::Owned(ival_data))
            }
        })
        .for_each(|i| intervals.push(i))
}

/// Materialize the [`RawInterval`] by stealing data from the data
fn materialize_raw_ref_intervals(
    raw_intervals: Vec<RawInterval>,
    data: &[u8],
    intervals: &mut Vec<Interval<RefIntervalData>>,
) {
    intervals.reserve(raw_intervals.len());
    raw_intervals
        .into_iter()
        .map(|raw| {
            if raw.is_empty() {
                Interval::default()
            } else if raw.is_zero() {
                Interval::new(raw.start, raw.end, RefIntervalData::Zero)
            } else {
                let len = raw.len();
                let data_begin = raw.offset as usize;
                let data_end = (raw.offset + len) as usize;
                let ival_data = data[data_begin..data_end].to_vec();
                Interval::new(raw.start, raw.end, RefIntervalData::Owned(ival_data))
            }
        })
        .for_each(|i| intervals.push(i))
}

#[cfg(test)]
mod test {
    use crate::deduper::Deduper;
    use crate::itree::ITree;

    use super::*;

    fn create_anon_from_zero(data: &[u8], virtual_range: (u64, u64)) -> ITree<AnonIntervalData> {
        let mut intervals = Vec::new();
        create_anon_itree_from_zero_page(data, virtual_range.0, &mut intervals);
        ITree::build(intervals, virtual_range).unwrap()
    }

    fn create_ref_from_zero(data: &[u8], virtual_range: (u64, u64)) -> ITree<RefIntervalData> {
        let mut intervals = Vec::new();
        create_ref_itree_from_zero_page(data, virtual_range.0, &mut intervals);
        ITree::build(intervals, virtual_range).unwrap()
    }

    fn create_from_diff(
        base: &[u8],
        overlay: &[u8],
        virtual_range: (u64, u64),
    ) -> ITree<RefIntervalData> {
        let mut intervals = Vec::new();
        create_itree_from_diff(base, overlay, virtual_range.0, &mut intervals);
        ITree::build(intervals, virtual_range).unwrap()
    }

    #[test]
    // test that it can create an interval tree no zero pages
    fn create_anon_zero_0() {
        let deduper = Deduper::default();
        let data = &[0xff; 0x1000 * 5];

        let itree = create_anon_from_zero(data, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![Interval::new(
                0x0000,
                0x5000,
                AnonIntervalData::Owned(vec![0xff; 0x1000 * 5]),
            )],
            (0x0000, 0x5000),
        )
        .unwrap();

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0);
        assert_eq!(itree.private_data_size(), 0x1000 * 5);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x3000),
            0x1000 * 3
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree all zero pages
    fn create_anon_zero_1() {
        let deduper = Deduper::default();
        let data = [0x00; 0x1000 * 5];

        let itree = create_anon_from_zero(&data, (0x0000, 0x5000));
        let target_itree = ITree::build(vec![], (0x0000, 0x5000)).unwrap();

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0);
        assert_eq!(itree.private_data_size(), 0);
        assert_eq!(itree.explicitely_mapped_subregion_size(0x0000, 0x3000), 0);

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree with a trailing zero range
    fn create_anon_zero_2() {
        let deduper = Deduper::default();
        let mut data = [0x00u8; 0x1000 * 5];
        data[0x0000..0x1000].fill(0xff);
        data[0x2000..0x3000].fill(0xff);

        let itree = create_anon_from_zero(&data, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![
                Interval::new(0x0000, 0x1000, AnonIntervalData::Owned(vec![0xff; 0x1000])),
                Interval::new(0x2000, 0x3000, AnonIntervalData::Owned(vec![0xff; 0x1000])),
            ],
            (0x0000, 0x5000),
        )
        .unwrap();

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0);
        assert_eq!(itree.private_data_size(), 0x1000 * 2);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x4000),
            0x1000 * 2
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree with a trailing data range
    fn create_anon_zero_3() {
        let deduper = Deduper::default();
        let mut data = [0x00u8; 0x1000 * 5];
        data[0x0000..0x1000].fill(0xff);
        data[0x3000..0x5000].fill(0xff);

        let itree = create_anon_from_zero(&data, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![
                Interval::new(0x0000, 0x1000, AnonIntervalData::Owned(vec![0xff; 0x1000])),
                Interval::new(0x3000, 0x5000, AnonIntervalData::Owned(vec![0xff; 0x2000])),
            ],
            (0x0000, 0x5000),
        )
        .unwrap();

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0);
        assert_eq!(itree.private_data_size(), 0x1000 * 3);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x4000),
            0x1000 * 2
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree no zero pages
    fn create_ref_zero_0() {
        let deduper = Deduper::default();
        let data = [0xff; 0x1000 * 5];

        let itree = create_ref_from_zero(&data, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![Interval::new(
                0x0000,
                0x5000,
                RefIntervalData::Owned(vec![0xff; 0x1000 * 5]),
            )],
            (0x0000, 0x5000),
        )
        .unwrap();

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0);
        assert_eq!(itree.private_data_size(), 0x1000 * 5);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x3000),
            0x1000 * 3
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree all zero pages
    fn create_ref_zero_1() {
        let deduper = Deduper::default();
        let data = [0x00; 0x1000 * 5];

        let itree = create_ref_from_zero(&data, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![Interval::new(0x0000, 0x5000, RefIntervalData::Zero)],
            (0x0000, 0x5000),
        )
        .unwrap();

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 5 * 0x1000);
        assert_eq!(itree.private_data_size(), 0);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x3000),
            3 * 0x1000
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree with a trailing zero range
    fn create_ref_zero_2() {
        let deduper = Deduper::default();
        let mut data = [0x00u8; 0x1000 * 5];
        data[0x0000..0x1000].fill(0xff);
        data[0x2000..0x3000].fill(0xff);

        let itree = create_ref_from_zero(&data, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![
                Interval::new(0x0000, 0x1000, RefIntervalData::Owned(vec![0xff; 0x1000])),
                Interval::new(0x1000, 0x2000, RefIntervalData::Zero),
                Interval::new(0x2000, 0x3000, RefIntervalData::Owned(vec![0xff; 0x1000])),
                Interval::new(0x3000, 0x5000, RefIntervalData::Zero),
            ],
            (0x0000, 0x5000),
        )
        .unwrap();

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 3 * 0x1000);
        assert_eq!(itree.private_data_size(), 0x1000 * 2);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x4000),
            4 * 0x1000
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree with a trailing data range
    fn create_ref_zero_3() {
        let deduper = Deduper::default();
        let mut data = [0x00u8; 0x1000 * 5];
        data[0x0000..0x1000].fill(0xff);
        data[0x3000..0x5000].fill(0xff);

        let itree = create_ref_from_zero(&data, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![
                Interval::new(0x0000, 0x1000, RefIntervalData::Owned(vec![0xff; 0x1000])),
                Interval::new(0x1000, 0x3000, RefIntervalData::Zero),
                Interval::new(0x3000, 0x5000, RefIntervalData::Owned(vec![0xff; 0x2000])),
            ],
            (0x0000, 0x5000),
        )
        .unwrap();

        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 2 * 0x1000);
        assert_eq!(itree.private_data_size(), 0x1000 * 3);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x4000),
            0x1000 * 4
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree when there is no difference
    fn create_diff_0() {
        let deduper = Deduper::default();
        let base = [0xffu8; 0x1000 * 5];
        let overlay = [0xffu8; 0x1000 * 5];

        let itree = create_from_diff(&base, &overlay, (0x0000, 0x5000));
        let target_itree = ITree::build(vec![], (0x0000, 0x5000)).unwrap();
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0);
        assert_eq!(itree.private_data_size(), 0);
        assert_eq!(itree.explicitely_mapped_subregion_size(0x0000, 0x5000), 0);
        assert_eq!(
            itree.implicitely_mapped_subregion_size(0x0000, 0x5000),
            0x1000 * 5
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree when there is no similarity
    fn create_diff_1() {
        let deduper = Deduper::default();
        let base = [0xffu8; 0x1000 * 5];
        let overlay = [0x88u8; 0x1000 * 5];

        let itree = create_from_diff(&base, &overlay, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![Interval::new(
                0x0000,
                0x5000,
                RefIntervalData::Owned(vec![0x88u8; 0x1000 * 5]),
            )],
            (0x0000, 0x5000),
        )
        .unwrap();
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0);
        assert_eq!(itree.private_data_size(), 0x1000 * 5);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x5000),
            0x1000 * 5
        );
        assert_eq!(itree.implicitely_mapped_subregion_size(0x0000, 0x5000), 0);

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next().unwrap(), vec![0x88; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0x88; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0x88; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0x88; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0x88; 0x1000]);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree when the overlay is zero
    fn create_diff_2() {
        let deduper = Deduper::default();
        let base = [0xffu8; 0x1000 * 5];
        let overlay = [0x00u8; 0x1000 * 5];

        let itree = create_from_diff(&base, &overlay, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![Interval::new(0x0000, 0x5000, RefIntervalData::Zero)],
            (0x0000, 0x5000),
        )
        .unwrap();
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 5);
        assert_eq!(itree.private_data_size(), 0);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x5000),
            0x1000 * 5
        );
        assert_eq!(itree.implicitely_mapped_subregion_size(0x0000, 0x5000), 0);

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next(), None);
    }

    #[test]
    // test that it can create an interval tree when the overlay is bigger than the base
    // include the fact that the overlay over-region may have zero pages
    fn create_diff_3() {
        let deduper = Deduper::default();
        let base = [0xffu8; 0x1000];
        let mut overlay = [0xffu8; 0x1000 * 5];
        overlay[0x4000..].fill(0x00);

        let itree = create_from_diff(&base, &overlay, (0x0000, 0x5000));
        let target_itree = ITree::build(
            vec![
                Interval::new(
                    0x1000,
                    0x4000,
                    RefIntervalData::Owned(vec![0xffu8; 0x1000 * 3]),
                ),
                Interval::new(0x4000, 0x5000, RefIntervalData::Zero),
            ],
            (0x0000, 0x5000),
        )
        .unwrap();
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000);
        assert_eq!(itree.private_data_size(), 0x1000 * 3);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x5000),
            0x1000 * 4
        );
        assert_eq!(
            itree.implicitely_mapped_subregion_size(0x0000, 0x5000),
            0x1000
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next(), None);
    }

    #[test]
    // complete test:
    //  - include overlay over-extension with trailing zeroes
    //  - include sparse pages
    fn create_diff_4() {
        let deduper = Deduper::default();
        let base = [0xffu8; 0x1000 * 6];
        let mut overlay = [0x00u8; 0x1000 * 10];
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

        let itree = create_from_diff(&base, &overlay, (0x0000, 0xa000));
        let target_itree = ITree::build(
            vec![
                Interval::new(0x1000, 0x3000, RefIntervalData::Zero),
                Interval::new(0x3000, 0x4000, RefIntervalData::Owned(vec![0xaa; 0x1000])),
                Interval::new(
                    0x5000,
                    0x7000,
                    RefIntervalData::Owned({
                        let mut v = vec![0x00; 0x1000 * 2];
                        v[..0x1000].fill(0xaa);
                        v[0x1000..].fill(0xff);
                        v
                    }),
                ),
                Interval::new(0x7000, 0x8000, RefIntervalData::Zero),
                Interval::new(0x8000, 0x9000, RefIntervalData::Owned(vec![0xff; 0x1000])),
                Interval::new(0x9000, 0xa000, RefIntervalData::Zero),
            ],
            (0x0000, 0xa000),
        )
        .unwrap();
        assert_eq!(itree.nodes, target_itree.nodes);
        assert_eq!(itree.zero_byte_size(), 0x1000 * 4);
        assert_eq!(itree.private_data_size(), 0x1000 * 4);
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0x5000),
            0x1000 * 3
        );
        assert_eq!(
            itree.implicitely_mapped_subregion_size(0x0000, 0x5000),
            0x1000 * 2
        );
        assert_eq!(
            itree.explicitely_mapped_subregion_size(0x0000, 0xa000),
            0x1000 * 8
        );
        assert_eq!(
            itree.implicitely_mapped_subregion_size(0x0000, 0xa000),
            0x1000 * 2
        );

        let mut it = itree.iter_private_pages(&deduper);
        assert_eq!(it.next().unwrap(), vec![0xaa; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xaa; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next().unwrap(), vec![0xff; 0x1000]);
        assert_eq!(it.next(), None);
    }
}
