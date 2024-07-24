//! Interval representation
use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::deduper::{DedupToken, Deduper};
use crate::error::{IntervalError, IntervalResult};

/// Data source resolved by the [`ITree`]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataSource {
    Zero,
    Shared,
    Private,
}

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
    Owned(Vec<u8>),
    Ref(DedupToken),

    #[default]
    None,
}

/// Data for a reference segment resolved by an interval
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum RefIntervalData {
    Owned(Vec<u8>),
    Ref(DedupToken),
    Zero,

    #[default]
    None,
}

impl From<&AnonIntervalData> for DataSource {
    fn from(value: &AnonIntervalData) -> Self {
        match value {
            AnonIntervalData::None => DataSource::Zero,
            AnonIntervalData::Owned(_) | AnonIntervalData::Ref(_) => DataSource::Private,
        }
    }
}

impl From<&RefIntervalData> for DataSource {
    fn from(value: &RefIntervalData) -> Self {
        match value {
            RefIntervalData::None => DataSource::Shared,
            RefIntervalData::Owned(_) | RefIntervalData::Ref(_) => DataSource::Private,
            RefIntervalData::Zero => DataSource::Zero,
        }
    }
}

/// Generic Interval Data
pub trait IntervalData {
    /// Check if it references the zero page
    fn is_zero(&self) -> bool;

    /// Check if it a non-existing interval
    fn is_none(&self) -> bool;

    /// Check if it has data
    fn is_data(&self) -> bool;

    /// Remove the data, if owned
    fn take_data(&mut self) -> Option<Vec<u8>>;

    /// View the data (whether owned or referenced)
    fn get_data<'a>(&'a self, deduper: &'a Deduper) -> Option<&'a [u8]>;
}

impl IntervalData for AnonIntervalData {
    fn is_zero(&self) -> bool {
        false
    }
    fn is_none(&self) -> bool {
        matches!(self, AnonIntervalData::None)
    }
    fn is_data(&self) -> bool {
        matches!(self, AnonIntervalData::Owned(_) | AnonIntervalData::Ref(_))
    }
    fn take_data(&mut self) -> Option<Vec<u8>> {
        if let AnonIntervalData::Owned(ref mut v) = self {
            Some(v.split_off(0))
        } else {
            None
        }
    }
    fn get_data<'a>(&'a self, deduper: &'a Deduper) -> Option<&'a [u8]> {
        if let AnonIntervalData::Owned(ref v) = self {
            Some(v)
        } else if let AnonIntervalData::Ref(token) = self {
            Some(deduper.get(*token))
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
        matches!(self, RefIntervalData::Owned(_) | RefIntervalData::Ref(_))
    }
    fn take_data(&mut self) -> Option<Vec<u8>> {
        if let RefIntervalData::Owned(ref mut v) = self {
            Some(v.split_off(0))
        } else {
            None
        }
    }
    fn get_data<'a>(&'a self, deduper: &'a Deduper) -> Option<&'a [u8]> {
        if let RefIntervalData::Owned(ref v) = self {
            Some(v)
        } else if let RefIntervalData::Ref(token) = self {
            Some(deduper.get(*token))
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

    /// Compare an address with this interval
    pub(crate) fn cmp(&self, addr: u64) -> Ordering {
        if addr < self.start {
            Ordering::Less
        } else if addr < self.end {
            Ordering::Equal
        } else {
            Ordering::Greater
        }
    }
}

impl Interval<AnonIntervalData> {
    /// Construct an interval from a raw interval
    pub(crate) fn from_raw_anon(
        raw: &RawInterval,
        data_offset: u64,
        deduper: &Deduper,
        offset_idx: &BTreeMap<(u64, u64), DedupToken>,
    ) -> IntervalResult<Self> {
        if raw.is_empty() {
            Ok(Interval::default())
        } else if raw.is_zero() {
            Err(IntervalError::ZeroIntervalInAnon)
        } else {
            let data_range = (
                raw.offset - data_offset,
                raw.offset + raw.len() - data_offset,
            );
            let priv_data_token = offset_idx
                .get(&data_range)
                .expect("by construction, the data map should have this data");
            let priv_data = deduper.get(*priv_data_token);
            assert_eq!(priv_data.len(), (raw.end - raw.start) as usize);
            let data = AnonIntervalData::Ref(*priv_data_token);
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
        deduper: &Deduper,
        offset_idx: &BTreeMap<(u64, u64), DedupToken>,
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
            let priv_data_token = offset_idx
                .get(&data_range)
                .expect("by construction, the data map should have this data");
            let priv_data = deduper.get(*priv_data_token);
            assert_eq!(priv_data.len(), (raw.end - raw.start) as usize);
            let data = RefIntervalData::Ref(*priv_data_token);
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
        deduper: &mut Deduper,
        token_map: &mut BTreeMap<DedupToken, (u64, u64)>,
        last_data_offset: &mut u64,
    ) -> Self {
        match interval.data {
            AnonIntervalData::None => RawInterval::default(),
            AnonIntervalData::Owned(interval_data) => {
                let data_len = interval_data.len() as u64;
                let token = deduper.insert(interval_data);
                let range = token_map.entry(token).or_insert_with(|| {
                    let range = (*last_data_offset, *last_data_offset + data_len);
                    *last_data_offset += data_len;
                    range
                });

                RawInterval {
                    start: interval.start,
                    end: interval.end,
                    offset: range.0,
                }
            }
            AnonIntervalData::Ref(token) => {
                let data_len = interval.len();
                let range = token_map.entry(token).or_insert_with(|| {
                    let range = (*last_data_offset, *last_data_offset + data_len);
                    *last_data_offset += data_len;
                    range
                });

                RawInterval {
                    start: interval.start,
                    end: interval.end,
                    offset: range.0,
                }
            }
        }
    }

    pub(crate) fn from_materialized_ref(
        interval: Interval<RefIntervalData>,
        deduper: &mut Deduper,
        token_map: &mut BTreeMap<DedupToken, (u64, u64)>,
        last_data_offset: &mut u64,
    ) -> Self {
        match interval.data {
            RefIntervalData::None => RawInterval::default(),
            RefIntervalData::Zero => RawInterval {
                start: interval.start,
                end: interval.end,
                offset: u64::MAX,
            },
            RefIntervalData::Owned(interval_data) => {
                let data_len = interval_data.len() as u64;
                let token = deduper.insert(interval_data);
                let range = token_map.entry(token).or_insert_with(|| {
                    let range = (*last_data_offset, *last_data_offset + data_len);
                    *last_data_offset += data_len;
                    range
                });

                RawInterval {
                    start: interval.start,
                    end: interval.end,
                    offset: range.0,
                }
            }
            RefIntervalData::Ref(token) => {
                let data_len = interval.len();
                let range = token_map.entry(token).or_insert_with(|| {
                    let range = (*last_data_offset, *last_data_offset + data_len);
                    *last_data_offset += data_len;
                    range
                });

                RawInterval {
                    start: interval.start,
                    end: interval.end,
                    offset: range.0,
                }
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
