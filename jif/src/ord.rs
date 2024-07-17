//! The ordering segments
use crate::jif::Jif;
use crate::utils::{is_page_aligned, page_align_down, PAGE_SIZE};

/// An ordering chunk represents a range of pages to pre-fault
#[derive(Debug, Default)]
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

            // n pages needs to fit in 12 bits
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
        assert!(is_page_aligned(vaddr));

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
