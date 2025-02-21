//! The ordering segments
use crate::itree::interval::DataSource;
use crate::jif::Jif;
use crate::utils::{page_align_down, PAGE_SIZE};

pub const ORD_SHARED_FLAG: u64 = 1 << 63;
pub const ORD_PRIVATE_FLAG: u64 = 1 << 62;
pub const ORD_ZERO_FLAG: u64 = 1 << 61;
pub const ORD_KIND_MASK: u64 = ORD_ZERO_FLAG - 1;
pub const ORD_WRITE_FLAG: u64 = 1 << 60;
pub const ORD_FLAG_MASK: u64 = ORD_WRITE_FLAG - 1;

/// An ordering chunk represents a range of pages to pre-fault
#[derive(PartialEq, Eq, Copy, Clone)]
pub struct OrdChunk {
    /// Timestamp of the access
    pub(crate) timestamp_us: u64,

    /// Page number of the first page
    pub(crate) vaddr: u64,

    /// Number of pages
    pub(crate) n_pages: u64,

    /// Source of the interval in the chunk
    pub(crate) kind: DataSource,

    /// Whether the page is written to at some point
    pub(crate) is_written_to: bool,
}

impl OrdChunk {
    /// The size of the [`OrdChunk`] when serialized on disk
    pub(crate) const fn serialized_size() -> usize {
        3 * std::mem::size_of::<u64>()
    }

    fn sanitize_addr(raw_vaddr: u64) -> u64 {
        page_align_down(raw_vaddr) & ORD_FLAG_MASK
    }

    fn raw_addr_written_to(raw_vaddr: u64) -> bool {
        (raw_vaddr & ORD_WRITE_FLAG) != 0
    }

    /// Create a new ordering chunk
    ///
    /// Will silently clamp the `vaddr`
    pub fn new(timestamp_us: u64, raw_vaddr: u64, n_pages: u64, kind: DataSource) -> Self {
        let is_written_to = Self::raw_addr_written_to(raw_vaddr);
        let vaddr = Self::sanitize_addr(raw_vaddr);
        OrdChunk {
            timestamp_us,
            vaddr,
            n_pages,
            kind,
            is_written_to,
        }
    }

    /// Whether this ordering chunk has any data
    pub fn is_empty(&self) -> bool {
        self.n_pages == 0
    }

    /// Number of pages in the ordering chunk
    pub fn size(&self) -> u64 {
        self.n_pages
    }

    /// Kind of ordering segment
    pub fn kind(&self) -> DataSource {
        self.kind
    }

    /// The address of the first page in the ordering chunk
    pub fn addr(&self) -> u64 {
        self.vaddr
    }

    /// The address of the last page in the ordering chunk
    pub fn last_page_addr(&self) -> u64 {
        if self.n_pages > 1 {
            self.vaddr + (self.n_pages - 1) * PAGE_SIZE as u64
        } else {
            self.vaddr
        }
    }

    /// The end of the memory region referenced by the ordering chunk
    pub fn end(&self) -> u64 {
        self.last_page_addr() + PAGE_SIZE as u64
    }

    /// First address of each page
    pub fn pages(&self) -> impl Iterator<Item = u64> {
        (self.vaddr..=(self.last_page_addr())).step_by(PAGE_SIZE)
    }

    /// Attempt to merge a page (`vaddr`) into the ordering chunk, which happens if:
    ///  - the page is contiguous to it (or is already in it)
    ///  - **and** they are serviced by the same pheader
    ///
    /// Return false if it is not possible to merge the page
    pub fn merge_page(&mut self, jif: &Jif, timestamp_us: u64, raw_vaddr: u64) -> bool {
        let vaddr = Self::sanitize_addr(raw_vaddr);
        let is_written_to = Self::raw_addr_written_to(raw_vaddr);

        if self.n_pages == 0 {
            self.vaddr = vaddr;
            self.n_pages = 1;
            self.is_written_to = is_written_to;
            return true;
        }

        // we can only merge addresses if they have the same write behaviour
        if is_written_to != self.is_written_to {
            return false;
        }

        // we can only merge if the addresses belong in the same itree
        // interval (logically) and, consequently, in the same pheader
        if jif.resolve(vaddr) != jif.resolve(self.vaddr) {
            return false;
        }

        if vaddr == self.vaddr - PAGE_SIZE as u64 {
            // if the page is immediately before the ordering chunk

            self.vaddr = vaddr;
            self.n_pages += 1;
            self.timestamp_us = std::cmp::min(self.timestamp_us, timestamp_us);
            true
        } else if vaddr == self.vaddr + (self.n_pages * PAGE_SIZE as u64) {
            // if the page is immediately after the ordering chunk

            self.n_pages += 1;
            self.timestamp_us = std::cmp::min(self.timestamp_us, timestamp_us);
            true
        } else if self.vaddr <= vaddr && vaddr < self.vaddr + (self.n_pages * PAGE_SIZE as u64) {
            // if the page is already in the ordering chunk
            self.timestamp_us = std::cmp::min(self.timestamp_us, timestamp_us);
            true
        } else {
            false
        }
    }

    pub(crate) fn split_by_intervals(&self, jif: &Jif) -> Vec<OrdChunk> {
        let mut cursor = self.vaddr;
        let mut ords = Vec::new();
        while cursor < self.last_page_addr() {
            let head = cursor;
            while cursor < self.last_page_addr() && jif.resolve(head) == jif.resolve(cursor) {
                cursor += PAGE_SIZE as u64;
            }

            let kind = jif
                .resolve(head)
                .expect("existing ord chunks should be mapped")
                .source;
            ords.push(OrdChunk {
                timestamp_us: self.timestamp_us,
                vaddr: head,
                n_pages: (cursor - head) / PAGE_SIZE as u64,
                kind,
                is_written_to: self.is_written_to,
            });
        }

        ords
    }
}

impl std::fmt::Debug for OrdChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ord")
            .field("vaddr", &self.vaddr)
            .field("n_pages", &self.n_pages)
            .field("kind", &self.kind)
            .field("timestamp_us", &self.timestamp_us)
            .field("is_written_to", &self.is_written_to)
            .finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::jif::test::gen_jif;

    #[test]
    fn empty_ord() {
        let ord = OrdChunk::new(0, 0x1234, 0, DataSource::Zero);
        assert_eq!(
            ord,
            OrdChunk {
                timestamp_us: 0,
                vaddr: 0x1000,
                n_pages: 0,
                kind: DataSource::Zero,
                is_written_to: false,
            }
        );
        assert!(ord.is_empty());
    }

    #[test]
    fn single_page_ord() {
        let ord = OrdChunk::new(0, 0x1234, 1, DataSource::Zero);
        assert_eq!(
            ord,
            OrdChunk {
                timestamp_us: 0,
                vaddr: 0x1000,
                n_pages: 1,
                kind: DataSource::Zero,
                is_written_to: false,
            }
        );
        assert!(!ord.is_empty());
        assert_eq!(ord.last_page_addr(), 0x1000);
    }

    #[test]
    fn multi_page_ord() {
        let ord = OrdChunk::new(0, 0x1234, 10, DataSource::Zero);
        assert_eq!(
            ord,
            OrdChunk {
                timestamp_us: 0,
                vaddr: 0x1000,
                n_pages: 10,
                kind: DataSource::Zero,
                is_written_to: false,
            }
        );
        assert!(!ord.is_empty());
        assert_eq!(ord.last_page_addr(), 0xa000);
    }

    #[test]
    fn merge_diff_sources() {
        let jif = gen_jif(&[
            ((0x10000, 0x20000), &[(0x10000, 0x18000)]),
            ((0x20000, 0x30000), &[(0x28000, 0x30000)]),
        ]);

        {
            let mut ord = OrdChunk::new(0, 0x11000, 0x6, DataSource::Zero);

            assert!(ord.merge_page(&jif, 0, 0x10000));
            assert_eq!(ord, OrdChunk::new(0, 0x10000, 0x7, DataSource::Zero));

            assert!(ord.merge_page(&jif, 0, 0x17000));
            assert_eq!(ord, OrdChunk::new(0, 0x10000, 0x8, DataSource::Zero));

            assert!(!ord.merge_page(&jif, 0, 0x1f000));
            assert_eq!(ord, OrdChunk::new(0, 0x10000, 0x8, DataSource::Zero));
        }

        {
            let mut ord = OrdChunk::new(0, 0x19000, 0x6, DataSource::Zero);

            assert!(ord.merge_page(&jif, 0, 0x18000));
            assert_eq!(ord, OrdChunk::new(0, 0x18000, 0x7, DataSource::Zero));

            assert!(!ord.merge_page(&jif, 0, 0x17000));

            assert!(ord.merge_page(&jif, 0, 0x1f000));
            assert_eq!(ord, OrdChunk::new(0, 0x18000, 0x8, DataSource::Zero));

            assert!(!ord.merge_page(&jif, 0, 0x20000));
        }
    }

    #[test]
    fn merge_same_sources() {
        let jif = gen_jif(&[((0x10000, 0x20000), &[]), ((0x20000, 0x30000), &[])]);

        {
            let mut ord = OrdChunk::new(0, 0x11000, 0xe, DataSource::Zero);

            assert!(ord.merge_page(&jif, 0, 0x10000));
            assert_eq!(ord, OrdChunk::new(0, 0x10000, 0xf, DataSource::Zero));

            assert!(ord.merge_page(&jif, 0, 0x17000));
            assert_eq!(ord, OrdChunk::new(0, 0x10000, 0xf, DataSource::Zero));

            assert!(ord.merge_page(&jif, 0, 0x1f000));
            assert_eq!(ord, OrdChunk::new(0, 0x10000, 0x10, DataSource::Zero));

            assert!(!ord.merge_page(&jif, 0, 0x20000));
            assert_eq!(ord, OrdChunk::new(0, 0x10000, 0x10, DataSource::Zero));
        }
    }

    #[test]
    fn merge_with_timestamp() {
        let jif = gen_jif(&[((0x10000, 0x20000), &[]), ((0x20000, 0x30000), &[])]);

        {
            let mut ord = OrdChunk::new(100, 0x11000, 0xe, DataSource::Zero);

            assert!(ord.merge_page(&jif, 150, 0x10000));
            assert_eq!(ord, OrdChunk::new(100, 0x10000, 0xf, DataSource::Zero));

            assert!(ord.merge_page(&jif, 50, 0x17000));
            assert_eq!(ord, OrdChunk::new(50, 0x10000, 0xf, DataSource::Zero));

            assert!(ord.merge_page(&jif, 100, 0x1f000));
            assert_eq!(ord, OrdChunk::new(50, 0x10000, 0x10, DataSource::Zero));

            assert!(!ord.merge_page(&jif, 0, 0x20000));
            assert_eq!(ord, OrdChunk::new(50, 0x10000, 0x10, DataSource::Zero));
        }
    }
}
