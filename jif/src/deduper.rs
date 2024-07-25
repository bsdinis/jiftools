//! Data deduplication logic

use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasher, Hash};

/// Tokens issued by a [`Deduper`]
///
/// This new-type ensures that unless there is a bug (i.e., re-using tokens
/// from a wrong deduper into the new one) any data request will succeed
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DedupToken(u64);

/// The data aggregator to de-duplicate data segments
///
/// This holds all the non-owned interval data and is used to deduplicate them
#[derive(Default)]
pub struct Deduper {
    /// map from data hash to the owned data
    canonical: HashMap<u64, Vec<u8>>,

    /// hash builder
    hash_builder: RandomState,
}

impl Deduper {
    pub(crate) fn with_capacity(n: usize) -> Self {
        Deduper {
            canonical: HashMap::with_capacity(n),
            hash_builder: RandomState::default(),
        }
    }

    pub(crate) fn from_data_map(
        data_map: BTreeMap<(u64, u64), Vec<u8>>,
    ) -> (Self, BTreeMap<(u64, u64), DedupToken>) {
        let mut deduper = Self::with_capacity(data_map.len());
        let mut offset_index = BTreeMap::new();

        for (range, data) in data_map {
            let token = deduper.insert(data);
            offset_index.insert(range, token);
        }

        (deduper, offset_index)
    }

    fn hash(&self, data: &[u8]) -> u64 {
        self.hash_builder.hash_one(data)
    }

    pub(crate) fn insert(&mut self, data: Vec<u8>) -> DedupToken {
        let token = self.hash(&data);
        if self.canonical.contains_key(&token) {
            return DedupToken(token);
        }

        self.canonical.insert(token, data);
        DedupToken(token)
    }

    pub(crate) fn get(&self, token: DedupToken) -> &[u8] {
        self.canonical.get(&token.0).map(|v| v.as_ref()).expect("by construction, requesting data from the deduper with a dedup token should always work")
    }

    pub(crate) fn destructure(
        mut self,
        token_map: BTreeMap<DedupToken, (u64, u64)>,
    ) -> BTreeMap<(u64, u64), Vec<u8>> {
        let intervals = {
            let mut v = token_map.into_iter().collect::<Vec<_>>();
            v.sort_by_key(|(_tok, range)| *range);
            v
        };

        let mut data_map = BTreeMap::new();
        let mut last_issued = intervals.first().map(|(_tok, range)| range.0).unwrap_or(0);
        for (tok, range) in intervals {
            assert_eq!(
                range.0, last_issued,
                "badly constructed data segment: there is a gap"
            );

            let data = self
                .canonical
                .remove(&tok.0)
                .expect("by construction, data should be here");
            data_map.insert(range, data);
            last_issued = range.1;
        }

        data_map
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn dedup_simple() {
        let mut deduper = Deduper::default();
        let token1 = deduper.insert(vec![0xa; 0x1000]);
        let token2 = deduper.insert(vec![0xa; 0x1000]);
        let token3 = deduper.insert(vec![0xb; 0x1000]);
        assert_eq!(token1, token2);
        assert_ne!(token1, token3);

        assert_eq!(deduper.get(token1), &[0xa; 0x1000]);
        assert_eq!(deduper.get(token2), &[0xa; 0x1000]);
        assert_eq!(deduper.get(token3), &[0xb; 0x1000]);
    }

    #[test]
    fn from_data_map() {
        let mut data_map = BTreeMap::new();
        data_map.insert((0x1000, 0x2000), vec![0x1; 0x1000]);
        data_map.insert((0x2000, 0x3000), vec![0x1; 0x1000]);
        data_map.insert((0x3000, 0x4000), vec![0x1; 0x1000]);

        let (deduper, offset_index) = Deduper::from_data_map(data_map.clone());
        assert_eq!(offset_index.len(), 3);
        assert_eq!(
            offset_index.get(&(0x1000, 0x2000)),
            offset_index.get(&(0x2000, 0x3000))
        );
        assert_eq!(
            offset_index.get(&(0x2000, 0x3000)),
            offset_index.get(&(0x3000, 0x4000))
        );

        let token = offset_index.get(&(0x1000, 0x2000)).unwrap();
        assert_eq!(deduper.get(*token), &[0x1; 0x1000]);
    }

    #[test]
    fn destructure() {
        let mut deduper = Deduper::default();
        let token1 = deduper.insert(vec![0xa; 0x1000]);
        let token2 = deduper.insert(vec![0xb; 0x1000]);

        let mut token_map = BTreeMap::new();
        token_map.insert(token1, (0x1000, 0x2000));
        token_map.insert(token2, (0x2000, 0x3000));

        let data_map = deduper.destructure(token_map);
        assert_eq!(data_map.len(), 2);
        assert_eq!(
            data_map.get(&(0x1000, 0x2000)).unwrap().as_slice(),
            &[0xa; 0x1000]
        );
        assert_eq!(
            data_map.get(&(0x2000, 0x3000)).unwrap().as_slice(),
            &[0xb; 0x1000]
        );
    }
}
