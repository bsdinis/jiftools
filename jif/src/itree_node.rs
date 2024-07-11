use std::collections::BTreeMap;

pub(crate) const FANOUT: usize = 4;
pub(crate) const IVAL_PER_NODE: usize = FANOUT - 1;

/// Node in a interval tree
///
/// Encodes a series of intervals
#[derive(Default, Clone, PartialEq, Eq)]
pub struct ITreeNode {
    pub(crate) ranges: [Interval; IVAL_PER_NODE],
}

/// Node in a raw interval tree
///
/// Encodes a series of raw intervals
#[derive(Default, Clone, PartialEq, Eq)]
pub struct RawITreeNode {
    pub(crate) ranges: [RawInterval; IVAL_PER_NODE],
}

/// Data resolved by an interval
#[derive(Default, Clone, PartialEq, Eq)]
pub enum IntervalData {
    Data(Vec<u8>),
    Zero,

    #[default]
    None,
}

/// Interval representation
///
/// We consider an interval valid if `start != u64::MAX` and `end != u64::MAX`
#[derive(Clone, PartialEq, Eq)]
pub struct Interval {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) data: IntervalData,
}

/// Raw interval representation
///
/// We consider an interval valid if `start != u64::MAX` and `end != u64::MAX`
/// If `offset == u64::MAX` it symbolizes that the interval references the zero page
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RawInterval {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) offset: u64,
}

impl ITreeNode {
    /// Build an `ITreeNode`
    pub(crate) fn from_raw(
        raw: &RawITreeNode,
        data_offset: u64,
        data_map: &mut BTreeMap<(u64, u64), Vec<u8>>,
    ) -> Self {
        let mut node = ITreeNode::default();
        for (raw_interval, interval) in raw.ranges.iter().zip(node.ranges.iter_mut()) {
            *interval = Interval::from_raw(raw_interval, data_offset, data_map);
        }
        node
    }

    /// Access the ranges within
    pub(crate) fn ranges(&self) -> &[Interval] {
        &self.ranges
    }

    /// Count the number of (non-empty) intervals
    pub(crate) fn n_intervals(&self) -> usize {
        self.ranges.iter().filter(|ival| !ival.is_none()).count()
    }

    /// Count the number of (non-empty) intervals
    pub(crate) fn n_data_intervals(&self) -> usize {
        self.ranges.iter().filter(|ival| ival.is_data()).count()
    }

    /// For this node, find how many virtual address space bytes are backed by the zero page
    pub(crate) fn zero_byte_size(&self) -> usize {
        self.ranges()
            .iter()
            .filter(|i| i.is_zero())
            .map(|i| i.len() as usize)
            .sum()
    }

    /// For this node, find how many virtual address space bytes are backed by the private data
    /// (contained in the JIF)
    pub(crate) fn private_data_size(&self) -> usize {
        self.ranges()
            .iter()
            .filter(|i| i.is_data())
            .map(|i| {
                debug_assert_eq!(
                    Some(i.len() as usize),
                    if let IntervalData::Data(ref d) = i.data {
                        Some(d.len())
                    } else {
                        None
                    }
                );
                i.len() as usize
            })
            .sum()
    }

    /// For this node, find how many virtual address space bytes are
    /// backed by private data or zero pages (i.e., are not backed by a reference segment) within
    /// a particular sub interval
    pub(crate) fn mapped_subregion_size(&self, start: u64, end: u64) -> usize {
        self.ranges()
            .iter()
            .filter(|i| !i.is_none())
            .filter_map(|i| i.intersect(start, end))
            .map(|(st, en)| (en - st) as usize)
            .sum()
    }
}

impl RawITreeNode {
    pub(crate) const fn serialized_size() -> usize {
        IVAL_PER_NODE * RawInterval::serialized_size()
    }

    /// Build an `ITreeNode`
    pub(crate) fn new(ranges: [RawInterval; IVAL_PER_NODE]) -> Self {
        RawITreeNode { ranges }
    }

    /// Lower an ITreeNode into a raw
    pub(crate) fn from_materialized(
        node: ITreeNode,
        data_base_offset: u64,
        data_size: &mut u64,
        data_map: &mut BTreeMap<(u64, u64), Vec<u8>>,
    ) -> Self {
        let mut raw = RawITreeNode::default();
        for (raw_interval, interval) in raw.ranges.iter_mut().zip(node.ranges.into_iter()) {
            *raw_interval =
                RawInterval::from_materialized(interval, data_base_offset, data_size, data_map);
        }
        raw
    }

    /// Access the ranges within
    pub(crate) fn ranges(&self) -> &[RawInterval] {
        &self.ranges
    }
}

impl Interval {
    /// Manually create an interval (for testing)
    pub(crate) fn new(start: u64, end: u64, data: IntervalData) -> Self {
        Interval { start, end, data }
    }

    /// Construct an interval from a raw interval
    pub(crate) fn from_raw(
        raw: &RawInterval,
        data_offset: u64,
        data_map: &mut BTreeMap<(u64, u64), Vec<u8>>,
    ) -> Self {
        if raw.is_empty() {
            Interval::default()
        } else if raw.is_zero() {
            Interval {
                start: raw.start,
                end: raw.end,
                data: IntervalData::Zero,
            }
        } else {
            let data_range = (
                raw.offset - data_offset,
                raw.offset + raw.len() - data_offset,
            );
            let priv_data = data_map
                .remove(&data_range)
                .expect("by construction, the data map should have this data");
            assert_eq!(priv_data.len(), (raw.end - raw.start) as usize);
            let data = IntervalData::Data(priv_data);
            Interval {
                start: raw.start,
                end: raw.end,
                data,
            }
        }
    }

    /// Check if the interval actually maps something or is just a stub
    pub(crate) fn is_none(&self) -> bool {
        self.start == u64::MAX || self.end == u64::MAX || matches!(self.data, IntervalData::None)
    }

    /// Check if the interval maps to the zero page
    pub(crate) fn is_zero(&self) -> bool {
        matches!(self.data, IntervalData::Zero)
    }

    /// Check if the interval maps to the private data
    pub(crate) fn is_data(&self) -> bool {
        matches!(self.data, IntervalData::Data(_))
    }

    /// Intersect the interval with another interval
    pub(crate) fn intersect(&self, start: u64, end: u64) -> Option<(u64, u64)> {
        if start >= end {
            return None;
        }

        let start = if start < self.start {
            self.start
        } else {
            std::cmp::min(start, self.end)
        };

        let end = if end > self.end {
            self.end
        } else {
            std::cmp::max(end, self.start)
        };

        (start < end).then_some((start, end))
    }

    /// Length of the mapped interval
    pub(crate) fn len(&self) -> u64 {
        if self.is_none() {
            0
        } else {
            self.end - self.start
        }
    }
}

impl RawInterval {
    pub(crate) const fn serialized_size() -> usize {
        3 * std::mem::size_of::<u64>()
    }

    pub(crate) fn new(start: u64, end: u64, offset: u64) -> Self {
        RawInterval { start, end, offset }
    }

    pub(crate) fn from_materialized(
        interval: Interval,
        data_base_offset: u64,
        data_size: &mut u64,
        data: &mut BTreeMap<(u64, u64), Vec<u8>>,
    ) -> Self {
        match interval.data {
            IntervalData::None => RawInterval::default(),
            IntervalData::Zero => RawInterval {
                start: interval.start,
                end: interval.end,
                offset: u64::MAX,
            },
            IntervalData::Data(interval_data) => {
                let interval = RawInterval {
                    start: interval.start,
                    end: interval.end,
                    offset: data_base_offset + *data_size as u64,
                };
                let data_len = interval_data.len() as u64;
                data.insert((*data_size, *data_size + data_len), interval_data);
                *data_size += data_len;
                interval
            }
        }
    }

    pub(crate) fn len(&self) -> u64 {
        if self.is_empty() {
            0
        } else {
            self.end - self.start
        }
    }

    /// Check if the interval is empty
    pub(crate) fn is_empty(&self) -> bool {
        self.start == u64::MAX || self.end == u64::MAX
    }

    /// Check if the interval maps to the zero page
    pub(crate) fn is_zero(&self) -> bool {
        self.offset == u64::MAX
    }

    /// Check if the interval points to private data
    pub(crate) fn is_data(&self) -> bool {
        !self.is_empty() && !self.is_zero()
    }
}

impl Default for Interval {
    fn default() -> Self {
        Interval {
            start: u64::MAX,
            end: u64::MAX,
            data: IntervalData::default(),
        }
    }
}

impl Default for RawInterval {
    fn default() -> Self {
        RawInterval {
            start: u64::MAX,
            end: u64::MAX,
            offset: u64::MAX,
        }
    }
}

impl std::fmt::Debug for ITreeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ITreeNode: ")?;
        f.debug_list()
            .entries(self.ranges.iter().filter(|i| !i.is_none()))
            .finish()
    }
}

impl std::fmt::Debug for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_none() {
            f.debug_struct("EmptyInterval").finish()
        } else if self.is_zero() {
            f.write_fmt(format_args!(
                "[{:#x}; {:#x}) -> <zero-page>",
                &self.start, &self.end
            ))
        } else {
            f.write_fmt(format_args!(
                "[{:#x}; {:#x}) -> <private-data: {:#x} B>",
                &self.start,
                &self.end,
                self.len()
            ))
        }
    }
}

impl std::fmt::Debug for RawITreeNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ITreeNode: ")?;
        f.debug_list()
            .entries(self.ranges.iter().filter(|i| !i.is_empty()))
            .finish()
    }
}

impl std::fmt::Debug for RawInterval {
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
    fn intersect() {
        let interval = Interval {
            start: 0x1000,
            end: 0x2000,
            data: IntervalData::Zero,
        };

        // non-intersecting
        assert_eq!(interval.intersect(0x0000, 0x1000), None);
        assert_eq!(interval.intersect(0x2000, 0x3000), None);

        // invalid interval
        assert_eq!(interval.intersect(0x2000, 0x1000), None);

        // empty interval
        assert_eq!(interval.intersect(0x1800, 0x1800), None);

        // full intersection
        assert_eq!(interval.intersect(0x1000, 0x2000), Some((0x1000, 0x2000)));

        // partial intersections
        assert_eq!(interval.intersect(0x0000, 0x2000), Some((0x1000, 0x2000)));
        assert_eq!(interval.intersect(0x0000, 0x3000), Some((0x1000, 0x2000)));
        assert_eq!(interval.intersect(0x1800, 0x2000), Some((0x1800, 0x2000)));
        assert_eq!(interval.intersect(0x1800, 0x3000), Some((0x1800, 0x2000)));
        assert_eq!(interval.intersect(0x0000, 0x1800), Some((0x1000, 0x1800)));
        assert_eq!(interval.intersect(0x1200, 0x1400), Some((0x1200, 0x1400)));
    }
}
