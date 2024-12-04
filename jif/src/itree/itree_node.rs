//! Nodes in the interval tree

use crate::deduper::{DedupToken, Deduper};
use crate::error::{ITreeNodeError, ITreeNodeResult};
use crate::itree::interval::{
    AnonIntervalData, IntermediateInterval, Interval, IntervalData, RawInterval, RefIntervalData,
};
use std::collections::BTreeMap;

pub(crate) const FANOUT: usize = 4;
pub(crate) const IVAL_PER_NODE: usize = FANOUT - 1;

/// Node in a [`crate::itree::ITree`]
///
/// Encodes a series of [`Interval`]s
#[derive(Default, Clone, PartialEq, Eq)]
pub struct ITreeNode<Data: IntervalData> {
    pub(crate) ranges: [Interval<Data>; IVAL_PER_NODE],
}

/// Intermediate node: holds [`IntermediateInterval`]s, before the data is ordered
#[derive(Default, Clone, PartialEq, Eq)]
pub struct IntermediateITreeNode {
    pub(crate) ranges: [IntermediateInterval; IVAL_PER_NODE],
}

/// Node in a raw interval tree
///
/// Encodes a series of [`RawInterval`]s
#[derive(Default, Clone, PartialEq, Eq)]
pub struct RawITreeNode {
    pub(crate) ranges: [RawInterval; IVAL_PER_NODE],
}

impl ITreeNode<AnonIntervalData> {
    /// Build an [`ITreeNode`] for an anonymous segment
    pub(crate) fn from_raw_anon(
        raw: &RawITreeNode,
        data_offset: u64,
        deduper: &Deduper,
        offset_idx: &BTreeMap<(u64, u64), DedupToken>,
    ) -> ITreeNodeResult<Self> {
        let mut node = ITreeNode::default();
        for (interval_idx, (raw_interval, interval)) in
            raw.ranges.iter().zip(node.ranges.iter_mut()).enumerate()
        {
            *interval = Interval::from_raw_anon(raw_interval, data_offset, deduper, offset_idx)
                .map_err(|interval_err| ITreeNodeError::Interval {
                    interval_idx,
                    interval_err,
                })?;
        }
        Ok(node)
    }
}

impl ITreeNode<RefIntervalData> {
    /// Build an [`ITreeNode`] for an reference segment
    pub(crate) fn from_raw_ref(
        raw: &RawITreeNode,
        data_offset: u64,
        deduper: &Deduper,
        offset_idx: &BTreeMap<(u64, u64), DedupToken>,
    ) -> Self {
        let mut node = ITreeNode::default();
        for (raw_interval, interval) in raw.ranges.iter().zip(node.ranges.iter_mut()) {
            *interval = Interval::from_raw_ref(raw_interval, data_offset, deduper, offset_idx);
        }
        node
    }
}

impl<Data: IntervalData> ITreeNode<Data> {
    /// construct single interval [`ITreeNode`]
    pub(crate) fn single(interval: Interval<Data>) -> ITreeNode<Data> {
        let mut node = ITreeNode::default();
        node.ranges[0] = interval;
        node
    }

    /// Access the ranges within
    pub(crate) fn ranges(&self) -> &[Interval<Data>] {
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
            .map(|i| i.len() as usize)
            .sum()
    }

    /// For this node, find how many virtual address space bytes are explicitely mapped within
    /// a particular sub interval
    pub(crate) fn explicitely_mapped_subregion_size(&self, start: u64, end: u64) -> usize {
        self.ranges()
            .iter()
            .filter(|i| !i.is_none())
            .filter_map(|i| i.intersect(start, end))
            .map(|(st, en)| (en - st) as usize)
            .sum()
    }
}

impl IntermediateITreeNode {
    /// Lower an anonymous ITreeNode into a raw
    pub(crate) fn from_materialized_anon(
        node: ITreeNode<AnonIntervalData>,
        deduper: &mut Deduper,
    ) -> Self {
        let mut inter = IntermediateITreeNode::default();
        for (inter_interval, interval) in inter.ranges.iter_mut().zip(node.ranges.into_iter()) {
            *inter_interval = IntermediateInterval::from_materialized_anon(interval, deduper);
        }
        inter
    }

    /// Lower a reference ITreeNode into a raw
    pub(crate) fn from_materialized_ref(
        node: ITreeNode<RefIntervalData>,
        deduper: &mut Deduper,
    ) -> Self {
        let mut inter = IntermediateITreeNode::default();
        for (inter_interval, interval) in inter.ranges.iter_mut().zip(node.ranges.into_iter()) {
            *inter_interval = IntermediateInterval::from_materialized_ref(interval, deduper);
        }
        inter
    }
}

impl RawITreeNode {
    /// Size of the [`RawITreeNode`] when serialized
    pub(crate) const fn serialized_size() -> usize {
        IVAL_PER_NODE * RawInterval::serialized_size()
    }

    /// Build an [`RawITreeNode`]
    pub(crate) fn new(ranges: [RawInterval; IVAL_PER_NODE]) -> Self {
        RawITreeNode { ranges }
    }

    /// Create a [`RawITreeNode`] from an [`IntermediateITreeNode`]
    /// This is done after serializing the data, so we already have the [`RawInterval`]s, but they
    /// are disorganized
    ///
    /// # Panics: this function panics if the interval is not present in `raw_intervals`
    pub(crate) fn from_intermediate(
        intermediate: IntermediateITreeNode,
        raw_intervals: &mut BTreeMap<(u64, u64), RawInterval>,
    ) -> Self {
        let mut raw = RawITreeNode::default();
        for (raw_interval, inter_interval) in
            raw.ranges.iter_mut().zip(intermediate.ranges.into_iter())
        {
            if inter_interval.is_none() {
                continue;
            }

            *raw_interval = raw_intervals.remove(&(inter_interval.start, inter_interval.end)).expect("cannot convert IntermediateInterval to RawInterval: `raw_intervals` is badly constructed");
        }
        raw
    }

    /// Access the ranges within
    pub(crate) fn ranges(&self) -> &[RawInterval] {
        &self.ranges
    }
}

impl<Data: IntervalData + std::fmt::Debug> std::fmt::Debug for ITreeNode<Data> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ITreeNode: ")?;
        f.debug_list()
            .entries(self.ranges.iter().filter(|i| !i.is_none()))
            .finish()
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
