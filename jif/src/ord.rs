use crate::jif::Jif;
use crate::utils::{is_page_aligned, page_align_down, PAGE_SIZE};

#[derive(Debug, Default)]
pub struct OrdChunk {
    /// first 42 bits encode the page number of the first page
    pub(crate) vaddr: u64,

    /// last 12 bits encode the number of pages
    pub(crate) n_pages: u16,
}

impl OrdChunk {
    pub(crate) const fn serialized_size() -> usize {
        std::mem::size_of::<u64>()
    }

    pub fn new(vaddr: u64, n_pages: u16) -> Self {
        OrdChunk {
            vaddr: page_align_down(vaddr),
            n_pages: std::cmp::min(n_pages, PAGE_SIZE as u16 - 1),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.n_pages == 0
    }

    pub fn last_page_addr(&self) -> u64 {
        if self.n_pages > 1 {
            self.vaddr + (self.n_pages - 1) as u64 * PAGE_SIZE as u64
        } else {
            self.vaddr
        }
    }

    /// merge a page if possible
    /// return false if it is not possible
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
            self.vaddr = vaddr;
            self.n_pages += 1;
            true
        } else if vaddr == self.vaddr + (self.n_pages as u64 * PAGE_SIZE as u64) {
            self.n_pages += 1;
            true
        } else {
            false
        }
    }
}
