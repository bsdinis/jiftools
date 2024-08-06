//! Immutable view over the interval tree

use crate::deduper::Deduper;
use crate::itree::interval::{AnonIntervalData, DataSource, LogicalInterval, RefIntervalData};
use crate::itree::ITree;

/// Generic view over the two possible types of [`ITree`]
pub enum ITreeView<'a> {
    /// Anonymous [`ITree`]
    Anon { inner: &'a ITree<AnonIntervalData> },

    /// Reference [`ITree`]
    Ref { inner: &'a ITree<RefIntervalData> },
}

impl<'a> ITreeView<'a> {
    /// Size of the [`ITree`] in number of nodes
    pub fn n_nodes(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.n_nodes(),
            ITreeView::Ref { inner } => inner.n_nodes(),
        }
    }

    /// Size of the [`ITree`] in number of intervals
    pub fn n_intervals(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.n_intervals(),
            ITreeView::Ref { inner } => inner.n_intervals(),
        }
    }

    /// Number of intervals holding data
    pub fn n_data_intervals(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.n_data_intervals(),
            ITreeView::Ref { inner } => inner.n_data_intervals(),
        }
    }

    /// Size of _explicit_ mappings to the zero page
    pub fn zero_byte_size(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.zero_byte_size(),
            ITreeView::Ref { inner } => inner.zero_byte_size(),
        }
    }

    /// Size of mappings to data
    pub fn private_data_size(&self) -> usize {
        match self {
            ITreeView::Anon { inner } => inner.private_data_size(),
            ITreeView::Ref { inner } => inner.private_data_size(),
        }
    }

    /// Iterate over the private pages in the interval tree
    pub fn iter_private_pages(
        &'a self,
        deduper: &'a Deduper,
    ) -> Box<dyn Iterator<Item = &[u8]> + 'a> {
        match self {
            ITreeView::Anon { inner } => Box::new(inner.iter_private_pages(deduper)),
            ITreeView::Ref { inner } => Box::new(inner.iter_private_pages(deduper)),
        }
    }

    /// Resolve address in the interval tree
    pub fn resolve(&self, addr: u64) -> LogicalInterval {
        match self {
            ITreeView::Anon { inner } => {
                match inner
                    .resolve(addr)
                    .map(|ival| ival.into())
                    .map_err(|(start, end)| LogicalInterval {
                        start,
                        end,
                        source: DataSource::Zero,
                    }) {
                    Ok(v) => v,
                    Err(v) => v,
                }
            }
            ITreeView::Ref { inner } => {
                match inner
                    .resolve(addr)
                    .map(|ival| ival.into())
                    .map_err(|(start, end)| LogicalInterval {
                        start,
                        end,
                        source: DataSource::Shared,
                    }) {
                    Ok(v) => v,
                    Err(v) => v,
                }
            }
        }
    }
}

impl<'a> std::fmt::Debug for ITreeView<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ITreeView::Anon { inner } => inner.fmt(f),
            ITreeView::Ref { inner } => inner.fmt(f),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::itree::test::*;

    #[test]
    fn anon_resolve_empty() {
        let itree: ITree<AnonIntervalData> = gen_empty();
        let view = ITreeView::Anon { inner: &itree };
        assert_eq!(view.resolve(0).source, DataSource::Zero);
        assert_eq!(view.resolve(VADDR_BEGIN).source, DataSource::Zero);
        assert_eq!(
            view.resolve((VADDR_BEGIN + VADDR_END) / 2).source,
            DataSource::Zero
        );
        assert_eq!(view.resolve(VADDR_END).source, DataSource::Zero);
    }

    #[test]
    fn ref_resolve_empty() {
        let itree: ITree<RefIntervalData> = gen_empty();
        let view = ITreeView::Ref { inner: &itree };
        assert_eq!(view.resolve(0).source, DataSource::Shared);
        assert_eq!(view.resolve(VADDR_BEGIN).source, DataSource::Shared);
        assert_eq!(
            view.resolve((VADDR_BEGIN + VADDR_END) / 2).source,
            DataSource::Shared
        );
        assert_eq!(view.resolve(VADDR_END).source, DataSource::Shared);
    }

    #[test]
    fn anon_resolve_filled() {
        let itree: ITree<AnonIntervalData> = gen_anon_tree();
        let view = ITreeView::Anon { inner: &itree };

        let mut cnt = 0;
        for addr in VADDRS
            .iter()
            .zip(VADDRS.iter().skip(1))
            .map(|(start, end)| (start + end) / 2)
        {
            let resolve = view.resolve(addr);
            match cnt % 2 {
                0 => assert_eq!(resolve.source, DataSource::Private),
                1 => assert_eq!(resolve.source, DataSource::Zero),
                _ => unreachable!(),
            };
            cnt += 1
        }
    }

    #[test]
    fn ref_resolve_filled() {
        let itree: ITree<RefIntervalData> = gen_ref_tree();
        let view = ITreeView::Ref { inner: &itree };

        let mut cnt = 0;
        for addr in VADDRS
            .iter()
            .zip(VADDRS.iter().skip(1))
            .map(|(start, end)| (start + end) / 2)
        {
            let resolve = view.resolve(addr);
            match cnt % 3 {
                0 => assert_eq!(resolve.source, DataSource::Private),
                1 => assert_eq!(resolve.source, DataSource::Zero),
                2 => assert_eq!(resolve.source, DataSource::Shared),
                _ => unreachable!(),
            };
            cnt += 1
        }
    }
}
