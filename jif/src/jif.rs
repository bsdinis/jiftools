use crate::error::*;
use crate::itree::ITreeNode;
use crate::ord::OrdChunk;
use crate::pheader::{JifPheader, JifRawPheader};

use std::io::{BufReader, Read, Seek};

pub(crate) const JIF_MAGIC_HEADER: [u8; 4] = [0x77, b'J', b'I', b'F'];

pub struct Jif {
    pub pheaders: Vec<JifPheader>,
    pub ord_chunks: Vec<OrdChunk>,
    pub data_offset: u64,
    pub data_segments: Vec<u8>,
}

/// JIF file representation
///
pub struct JifRaw {
    pub(crate) pheaders: Vec<JifRawPheader>,
    pub(crate) strings_backing: Vec<u8>,
    pub(crate) itree_nodes: Vec<ITreeNode>,
    pub(crate) ord_chunks: Vec<OrdChunk>,
    pub(crate) data_offset: u64,
    pub(crate) data_segments: Vec<u8>,
}

impl Jif {
    pub fn from_raw(raw: JifRaw) -> Self {
        let pheaders = raw
            .pheaders
            .iter()
            .map(|raw_pheader| JifPheader::from_raw(&raw, &raw_pheader))
            .collect::<Vec<JifPheader>>();

        Jif {
            pheaders,
            ord_chunks: raw.ord_chunks,
            data_offset: raw.data_offset,
            data_segments: raw.data_segments,
        }
    }

    pub fn from_reader<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<Self> {
        Ok(Jif::from_raw(JifRaw::from_reader(r)?))
    }
}

impl std::fmt::Debug for Jif {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Jif")
            .field("pheaders", &self.pheaders)
            .field("ord", &self.ord_chunks)
            .field("data_offset", &self.data_offset)
            .finish()
    }
}

impl std::fmt::Debug for JifRaw {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let strings = self.strings();
        f.debug_struct("Jif")
            .field("pheaders", &self.pheaders)
            .field("strings", &strings)
            .field("itrees", &self.itree_nodes)
            .field("ord", &self.ord_chunks)
            .field("data_offset", &self.data_offset)
            .finish()
    }
}
