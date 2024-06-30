#[derive(Debug)]
pub struct OrdChunk {
    /// first 42 bits encode the page number of the first page
    pub vaddr: u64,

    /// last 12 bits encode the number of pages
    pub n_pages: u16,
}

impl OrdChunk {
    pub(crate) const fn serialized_size() -> usize {
        std::mem::size_of::<u64>()
    }

    pub(crate) fn new(vaddr: u64, n_pages: u16) -> Self {
        OrdChunk { vaddr, n_pages }
    }
}
