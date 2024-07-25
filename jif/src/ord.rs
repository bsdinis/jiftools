//! The ordering segments
use crate::jif::Jif;
use crate::utils::{page_align_down, PAGE_SIZE};

/// An ordering chunk represents a range of pages to pre-fault
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OrdChunk {
    /// Page number of the first page
    pub(crate) vaddr: u64,

    /// Number of pages
    pub(crate) n_pages: u64,
}

impl OrdChunk {
    /// The size of the [`OrdChunk`] when serialized on disk
    pub(crate) const fn serialized_size() -> usize {
        2 * std::mem::size_of::<u64>()
    }

    /// Create a new ordering chunk
    ///
    /// Will silently clamp the `vaddr`
    pub fn new(vaddr: u64, n_pages: u64) -> Self {
        OrdChunk {
            vaddr: page_align_down(vaddr),

            n_pages,
        }
    }

    /// Whether this ordering chunk has any data
    pub fn is_empty(&self) -> bool {
        self.n_pages == 0
    }

    /// The address of the last page in the ordering chunk
    pub fn last_page_addr(&self) -> u64 {
        if self.n_pages > 1 {
            self.vaddr + (self.n_pages - 1) * PAGE_SIZE as u64
        } else {
            self.vaddr
        }
    }

    /// Attempt to merge a page (`vaddr`) into the ordering chunk, which happens if:
    ///  - the page is contiguous to it (or is already in it)
    ///  - **and** they are serviced by the same pheader
    ///
    /// Return false if it is not possible to merge the page
    pub fn merge_page(&mut self, jif: &Jif, vaddr: u64) -> bool {
        let vaddr = page_align_down(vaddr);

        if self.n_pages == 0 {
            self.vaddr = vaddr;
            self.n_pages = 1;
            return true;
        }

        if jif.mapping_pheader_idx(vaddr) != jif.mapping_pheader_idx(self.vaddr) {
            return false;
        }

        if vaddr == self.vaddr - PAGE_SIZE as u64 {
            // if the page is immediately before the ordering chunk

            self.vaddr = vaddr;
            self.n_pages += 1;
            true
        } else if vaddr == self.vaddr + (self.n_pages * PAGE_SIZE as u64) {
            // if the page is immediately after the ordering chunk

            self.n_pages += 1;
            true
        } else if self.vaddr <= vaddr && vaddr < self.vaddr + (self.n_pages * PAGE_SIZE as u64) {
            // if the page is already in the ordering chunk

            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::jif::test::gen_jif;

    #[test]
    fn empty_ord() {
        let ord = OrdChunk::new(0x1234, 0);
        assert_eq!(
            ord,
            OrdChunk {
                vaddr: 0x1000,
                n_pages: 0
            }
        );
        assert!(ord.is_empty());
    }

    #[test]
    fn single_page_ord() {
        let ord = OrdChunk::new(0x1234, 1);
        assert_eq!(
            ord,
            OrdChunk {
                vaddr: 0x1000,
                n_pages: 1
            }
        );
        assert!(!ord.is_empty());
        assert_eq!(ord.last_page_addr(), 0x1000);
    }

    #[test]
    fn multi_page_ord() {
        let ord = OrdChunk::new(0x1234, 10);
        assert_eq!(
            ord,
            OrdChunk {
                vaddr: 0x1000,
                n_pages: 10
            }
        );
        assert!(!ord.is_empty());
        assert_eq!(ord.last_page_addr(), 0xa000);
    }

    #[test]
    fn merge() {
        let jif = gen_jif(&[(0x10000, 0x20000), (0x20000, 0x30000)]);
        let mut ord = OrdChunk::new(0x11000, 0xe);

        assert!(ord.merge_page(&jif, 0x10000));
        assert_eq!(ord, OrdChunk::new(0x10000, 0xf));

        assert!(ord.merge_page(&jif, 0x1f000));
        assert_eq!(ord, OrdChunk::new(0x10000, 0x10));

        assert!(!ord.merge_page(&jif, 0x20000));
        assert_eq!(ord, OrdChunk::new(0x10000, 0x10));
    }
}
