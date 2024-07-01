pub(crate) const FANOUT: usize = 4;
pub(crate) const IVAL_PER_NODE: usize = FANOUT - 1;

pub struct ITree {
    pub(crate) nodes: Vec<ITreeNode>,
}

#[derive(Clone)]
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
pub(crate) struct Interval {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) offset: u64,
}

impl ITree {
    pub fn new(nodes: Vec<ITreeNode>) -> Self {
        ITree { nodes }
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
