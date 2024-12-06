//! Interval representation
use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::deduper::{DedupToken, Deduper};
use crate::error::{IntervalError, IntervalResult};

/// A logical interval, i.e.: what the generic interval tree resolves to
///
/// Internally, the interval tree can resolve to nothing (i.e., the resolution is surmised from the
/// itree type). However, it is generally useful to understand what the implied interval is
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogicalInterval {
    pub start: u64,
    pub end: u64,
    pub source: DataSource,
}

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

/// Intermediate representation for intervals
///
/// This is done so we can first extract the interval from a materialized ITree
/// And then, independently, order the data segments
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct IntermediateInterval {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) data: IntermediateIntervalData,
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

/// Data for an intermediate interval
///
/// Note that there are no more owned intervals, so this can be [`Copy`]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntermediateIntervalData {
    Ref(DedupToken),
    Zero,

    #[default]
    None,
}

/// Generic Interval Data
pub trait IntervalData: Default + Clone + From<Vec<u8>> {
    /// Check if it references the zero page
    fn is_zero(&self) -> bool;

    /// Check if it a non-existing interval
    fn is_none(&self) -> bool;

    /// Check if it has data
    fn is_data(&self) -> bool;

    /// Check if the data is owned
    fn is_owned(&self) -> bool;

    /// Check if the data is referenced
    fn is_ref(&self) -> bool;

    /// Remove the data, if owned
    fn take_data(&mut self) -> Option<Vec<u8>>;

    /// Dedup data, if owned
    fn dedup(&mut self, deduper: &mut Deduper);

    /// View the data (whether owned or referenced)
    fn get_data<'a>(&'a self, deduper: &'a Deduper) -> Option<&'a [u8]>;

    /// Return the dedup token if it is a reference
    fn dedup_token(&self) -> Option<DedupToken>;
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
    fn is_owned(&self) -> bool {
        matches!(self, AnonIntervalData::Owned(_))
    }
    fn is_ref(&self) -> bool {
        matches!(self, AnonIntervalData::Ref(_))
    }
    fn take_data(&mut self) -> Option<Vec<u8>> {
        if let AnonIntervalData::Owned(ref mut v) = self {
            Some(v.split_off(0))
        } else {
            None
        }
    }
    fn dedup(&mut self, deduper: &mut Deduper) {
        if let AnonIntervalData::Owned(ref mut v) = self {
            let data = v.split_off(0);
            let token = deduper.insert(data);
            *self = AnonIntervalData::Ref(token);
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
    fn dedup_token(&self) -> Option<DedupToken> {
        if let AnonIntervalData::Ref(token) = self {
            Some(*token)
        } else {
            None
        }
    }
}

impl From<Vec<u8>> for AnonIntervalData {
    fn from(value: Vec<u8>) -> Self {
        AnonIntervalData::Owned(value)
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
    fn is_owned(&self) -> bool {
        matches!(self, RefIntervalData::Owned(_))
    }
    fn is_ref(&self) -> bool {
        matches!(self, RefIntervalData::Ref(_))
    }
    fn take_data(&mut self) -> Option<Vec<u8>> {
        if let RefIntervalData::Owned(ref mut v) = self {
            Some(v.split_off(0))
        } else {
            None
        }
    }
    fn dedup(&mut self, deduper: &mut Deduper) {
        if let RefIntervalData::Owned(ref mut v) = self {
            let data = v.split_off(0);
            let token = deduper.insert(data);
            *self = RefIntervalData::Ref(token);
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
    fn dedup_token(&self) -> Option<DedupToken> {
        if let RefIntervalData::Ref(token) = self {
            Some(*token)
        } else {
            None
        }
    }
}

impl From<Vec<u8>> for RefIntervalData {
    fn from(value: Vec<u8>) -> Self {
        RefIntervalData::Owned(value)
    }
}

impl IntervalData for IntermediateIntervalData {
    fn is_zero(&self) -> bool {
        matches!(self, IntermediateIntervalData::Zero)
    }
    fn is_none(&self) -> bool {
        matches!(self, IntermediateIntervalData::None)
    }
    fn is_data(&self) -> bool {
        matches!(self, IntermediateIntervalData::Ref(_))
    }
    fn is_owned(&self) -> bool {
        false
    }
    fn is_ref(&self) -> bool {
        matches!(self, IntermediateIntervalData::Ref(_))
    }
    fn take_data(&mut self) -> Option<Vec<u8>> {
        None
    }
    fn dedup(&mut self, _deduper: &mut Deduper) {}
    fn get_data<'a>(&'a self, deduper: &'a Deduper) -> Option<&'a [u8]> {
        if let IntermediateIntervalData::Ref(token) = self {
            Some(deduper.get(*token))
        } else {
            None
        }
    }
    fn dedup_token(&self) -> Option<DedupToken> {
        if let IntermediateIntervalData::Ref(token) = self {
            Some(*token)
        } else {
            None
        }
    }
}

impl From<&Interval<AnonIntervalData>> for LogicalInterval {
    fn from(value: &Interval<AnonIntervalData>) -> Self {
        LogicalInterval {
            start: value.start,
            end: value.end,
            source: (&value.data).into(),
        }
    }
}

impl From<&Interval<RefIntervalData>> for LogicalInterval {
    fn from(value: &Interval<RefIntervalData>) -> Self {
        LogicalInterval {
            start: value.start,
            end: value.end,
            source: (&value.data).into(),
        }
    }
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

impl IntermediateInterval {
    /// Length of the mapped interval
    pub(crate) fn len(&self) -> u64 {
        if self.is_none() {
            0
        } else {
            self.end - self.start
        }
    }

    /// Check if the interval actually maps something or is just a stub
    pub(crate) fn is_none(&self) -> bool {
        self.start == u64::MAX || self.end == u64::MAX || self.data.is_none()
    }

    /// Check if the interval maps to the zero page
    pub(crate) fn is_zero(&self) -> bool {
        self.data.is_zero()
    }

    pub(crate) fn from_materialized_anon(
        interval: Interval<AnonIntervalData>,
        deduper: &mut Deduper,
    ) -> Self {
        match interval.data {
            AnonIntervalData::None => IntermediateInterval::default(),
            AnonIntervalData::Owned(interval_data) => {
                let token = deduper.insert(interval_data);

                IntermediateInterval {
                    start: interval.start,
                    end: interval.end,
                    data: IntermediateIntervalData::Ref(token),
                }
            }
            AnonIntervalData::Ref(token) => IntermediateInterval {
                start: interval.start,
                end: interval.end,
                data: IntermediateIntervalData::Ref(token),
            },
        }
    }

    pub(crate) fn from_materialized_ref(
        interval: Interval<RefIntervalData>,
        deduper: &mut Deduper,
    ) -> Self {
        match interval.data {
            RefIntervalData::None => IntermediateInterval::default(),
            RefIntervalData::Zero => IntermediateInterval {
                start: interval.start,
                end: interval.end,
                data: IntermediateIntervalData::Zero,
            },
            RefIntervalData::Owned(interval_data) => {
                let token = deduper.insert(interval_data);

                IntermediateInterval {
                    start: interval.start,
                    end: interval.end,
                    data: IntermediateIntervalData::Ref(token),
                }
            }
            RefIntervalData::Ref(token) => IntermediateInterval {
                start: interval.start,
                end: interval.end,
                data: IntermediateIntervalData::Ref(token),
            },
        }
    }
}

impl From<Vec<u8>> for IntermediateIntervalData {
    fn from(_value: Vec<u8>) -> Self {
        panic!("tried to convert an owned piece of interval data into an IntermediateIntervalData");
    }
}

impl RawInterval {
    pub(crate) const fn serialized_size() -> usize {
        3 * std::mem::size_of::<u64>()
    }

    pub(crate) fn new(start: u64, end: u64, offset: u64) -> Self {
        RawInterval { start, end, offset }
    }

    pub(crate) fn len(&self) -> u64 {
        if self.is_empty() {
            0
        } else {
            self.end - self.start
        }
    }

    pub(crate) fn from_intermediate(
        inter: &IntermediateInterval,
        token_map: &mut BTreeMap<DedupToken, (u64, u64)>,
        data_offset: &mut u64,
    ) -> Self {
        match inter.data {
            IntermediateIntervalData::None => RawInterval::default(),
            IntermediateIntervalData::Zero => RawInterval {
                start: inter.start,
                end: inter.end,
                offset: u64::MAX,
            },
            IntermediateIntervalData::Ref(token) => {
                let data_len = inter.len();
                let range = token_map.entry(token).or_insert_with(|| {
                    let range = (*data_offset, *data_offset + data_len);
                    *data_offset += data_len;
                    range
                });

                assert_eq!(data_len, range.1 - range.0, "interval [{:#x?}-{:#x?}] (size = {:#x?}) has a deduplication token ({:?}) with a diferent size: {:#x?}",
                    inter.start, inter.end, data_len, token, range.1 - range.0);

                RawInterval {
                    start: inter.start,
                    end: inter.end,
                    offset: range.0,
                }
            }
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

impl Default for IntermediateInterval {
    fn default() -> Self {
        IntermediateInterval {
            start: u64::MAX,
            end: u64::MAX,
            data: IntermediateIntervalData::default(),
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

impl std::fmt::Debug for IntermediateInterval {
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

impl TryFrom<RefIntervalData> for AnonIntervalData {
    type Error = ();
    fn try_from(value: RefIntervalData) -> Result<Self, Self::Error> {
        match value {
            RefIntervalData::Zero => Ok(AnonIntervalData::None),
            RefIntervalData::Ref(tok) => Ok(AnonIntervalData::Ref(tok)),
            RefIntervalData::Owned(data) => Ok(AnonIntervalData::Owned(data)),
            RefIntervalData::None => Err(()),
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
