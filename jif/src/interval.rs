use std::collections::BTreeMap;

use crate::error::{ITreeNodeError, IntervalError, JifError, JifResult};

/// Interval representation
///
/// We consider an interval valid if `start != u64::MAX` and `end != u64::MAX`
#[derive(Clone, PartialEq, Eq)]
pub struct Interval<Data: IntervalData> {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) data: Data,
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

/// Data for an anonymous segment resolved by an interval
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum AnonIntervalData {
    Data(Vec<u8>),

    #[default]
    None,
}

/// Data for a reference segment resolved by an interval
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum RefIntervalData {
    Data(Vec<u8>),
    Zero,

    #[default]
    None,
}

/// Generic Interval Data
pub trait IntervalData {
    fn is_zero(&self) -> bool;
    fn is_none(&self) -> bool;
    fn is_data(&self) -> bool;
    fn take_data(&mut self) -> Option<Vec<u8>>;
}

impl IntervalData for AnonIntervalData {
    fn is_zero(&self) -> bool {
        false
    }
    fn is_none(&self) -> bool {
        matches!(self, AnonIntervalData::None)
    }
    fn is_data(&self) -> bool {
        matches!(self, AnonIntervalData::Data(_))
    }
    fn take_data(&mut self) -> Option<Vec<u8>> {
        if let AnonIntervalData::Data(ref mut v) = self {
            Some(v.split_off(0))
        } else {
            None
        }
    }
}

impl IntervalData for RefIntervalData {
    fn is_zero(&self) -> bool {
        matches!(self, RefIntervalData::Zero)
    }
    fn is_none(&self) -> bool {
        matches!(self, RefIntervalData::None)
    }
    fn is_data(&self) -> bool {
        matches!(self, RefIntervalData::Data(_))
    }
    fn take_data(&mut self) -> Option<Vec<u8>> {
        if let RefIntervalData::Data(ref mut v) = self {
            Some(v.split_off(0))
        } else {
            None
        }
    }
}

impl<Data: IntervalData> Interval<Data> {
    /// Manually create an interval (for testing)
    pub(crate) fn new(start: u64, end: u64, data: Data) -> Self {
        Interval { start, end, data }
    }

    /// Check if the interval actually maps something or is just a stub
    pub(crate) fn is_none(&self) -> bool {
        self.start == u64::MAX || self.end == u64::MAX || self.data.is_none()
    }

    /// Check if the interval maps to the zero page
    pub(crate) fn is_zero(&self) -> bool {
        self.data.is_zero()
    }

    /// Check if the interval maps to the private data
    pub(crate) fn is_data(&self) -> bool {
        self.data.is_data()
    }

    /// Take ownership of the underlying data (if it exists)
    pub(crate) fn take_data(&mut self) -> Option<Vec<u8>> {
        self.data.take_data()
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

impl Interval<AnonIntervalData> {
    /// Construct an interval from a raw interval
    pub(crate) fn from_raw_anon(
        raw: &RawInterval,
        data_offset: u64,
        data_map: &mut BTreeMap<(u64, u64), Vec<u8>>,
        interval_idx: usize,
        itree_node_idx: usize,
    ) -> JifResult<Self> {
        if raw.is_empty() {
            Ok(Interval::default())
        } else if raw.is_zero() {
            Err(JifError::BadITreeNode {
                itree_node_idx,
                itree_node_err: ITreeNodeError {
                    interval_idx,
                    interval_err: IntervalError::ZeroIntervalInAnon,
                },
            })
        } else {
            let data_range = (
                raw.offset - data_offset,
                raw.offset + raw.len() - data_offset,
            );
            let priv_data = data_map
                .remove(&data_range)
                .expect("by construction, the data map should have this data");
            assert_eq!(priv_data.len(), (raw.end - raw.start) as usize);
            let data = AnonIntervalData::Data(priv_data);
            Ok(Interval {
                start: raw.start,
                end: raw.end,
                data,
            })
        }
    }
}

impl Interval<RefIntervalData> {
    /// Construct an interval from a raw interval
    pub(crate) fn from_raw_ref(
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
                data: RefIntervalData::Zero,
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
            let data = RefIntervalData::Data(priv_data);
            Interval {
                start: raw.start,
                end: raw.end,
                data,
            }
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

    pub(crate) fn from_materialized_anon(
        interval: Interval<AnonIntervalData>,
        data_base_offset: u64,
        data_size: &mut u64,
        data: &mut BTreeMap<(u64, u64), Vec<u8>>,
    ) -> Self {
        match interval.data {
            AnonIntervalData::None => RawInterval::default(),
            AnonIntervalData::Data(interval_data) => {
                let interval = RawInterval {
                    start: interval.start,
                    end: interval.end,
                    offset: data_base_offset + *data_size,
                };
                let data_len = interval_data.len() as u64;
                data.insert((*data_size, *data_size + data_len), interval_data);
                *data_size += data_len;
                interval
            }
        }
    }

    pub(crate) fn from_materialized_ref(
        interval: Interval<RefIntervalData>,
        data_base_offset: u64,
        data_size: &mut u64,
        data: &mut BTreeMap<(u64, u64), Vec<u8>>,
    ) -> Self {
        match interval.data {
            RefIntervalData::None => RawInterval::default(),
            RefIntervalData::Zero => RawInterval {
                start: interval.start,
                end: interval.end,
                offset: u64::MAX,
            },
            RefIntervalData::Data(interval_data) => {
                let interval = RawInterval {
                    start: interval.start,
                    end: interval.end,
                    offset: data_base_offset + *data_size,
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

impl<Data: IntervalData + Default> Default for Interval<Data> {
    fn default() -> Self {
        Interval {
            start: u64::MAX,
            end: u64::MAX,
            data: Data::default(),
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

impl<Data: IntervalData + std::fmt::Debug> std::fmt::Debug for Interval<Data> {
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
            data: RefIntervalData::Zero,
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
