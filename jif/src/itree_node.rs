use crate::deduper::{DedupToken, Deduper};
use crate::error::JifResult;
use crate::interval::{AnonIntervalData, Interval, IntervalData, RawInterval, RefIntervalData};
use std::collections::BTreeMap;

pub(crate) const FANOUT: usize = 4;
pub(crate) const IVAL_PER_NODE: usize = FANOUT - 1;

/// Node in a interval tree
///
/// Encodes a series of intervals
#[derive(Default, Clone, PartialEq, Eq)]
pub struct ITreeNode<Data: IntervalData> {
    pub(crate) ranges: [Interval<Data>; IVAL_PER_NODE],
}

/// Node in a raw interval tree
///
/// Encodes a series of raw intervals
#[derive(Default, Clone, PartialEq, Eq)]
pub struct RawITreeNode {
    pub(crate) ranges: [RawInterval; IVAL_PER_NODE],
}

impl ITreeNode<AnonIntervalData> {
    /// Build an `ITreeNode` for an anonymous segment
    pub(crate) fn from_raw_anon(
        raw: &RawITreeNode,
        data_offset: u64,
        deduper: &Deduper,
        offset_idx: &BTreeMap<(u64, u64), DedupToken>,
        itree_node_idx: usize,
    ) -> JifResult<Self> {
        let mut node = ITreeNode::default();
        for (interval_idx, (raw_interval, interval)) in
            raw.ranges.iter().zip(node.ranges.iter_mut()).enumerate()
        {
            *interval = Interval::from_raw_anon(
                raw_interval,
                data_offset,
                deduper,
                offset_idx,
                interval_idx,
                itree_node_idx,
            )?;
        }
        Ok(node)
    }
}

impl ITreeNode<RefIntervalData> {
    /// Build an `ITreeNode` for an reference segment
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
    // TODO(ref/anon): check if this makes sense here
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

    /// Lower an anonymous ITreeNode into a raw
    pub(crate) fn from_materialized_anon(
        node: ITreeNode<AnonIntervalData>,
        deduper: &mut Deduper,
        token_map: &mut BTreeMap<DedupToken, (u64, u64)>,
        last_data_offset: &mut u64,
    ) -> Self {
        let mut raw = RawITreeNode::default();
        for (raw_interval, interval) in raw.ranges.iter_mut().zip(node.ranges.into_iter()) {
            *raw_interval =
                RawInterval::from_materialized_anon(interval, deduper, token_map, last_data_offset);
        }
        raw
    }

    /// Lower a reference ITreeNode into a raw
    pub(crate) fn from_materialized_ref(
        node: ITreeNode<RefIntervalData>,
        deduper: &mut Deduper,
        token_map: &mut BTreeMap<DedupToken, (u64, u64)>,
        last_data_offset: &mut u64,
    ) -> Self {
        let mut raw = RawITreeNode::default();
        for (raw_interval, interval) in raw.ranges.iter_mut().zip(node.ranges.into_iter()) {
            *raw_interval =
                RawInterval::from_materialized_ref(interval, deduper, token_map, last_data_offset);
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
