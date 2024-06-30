#![allow(unused)]

use crate::error::*;
use std::collections::BTreeMap;
use std::io::{BufReader, Read, Seek};
use std::str::from_utf8;

const FANOUT: usize = 4;
const IVAL_PER_NODE: usize = FANOUT - 1;

pub(crate) const JIF_MAGIC_HEADER: [u8; 4] = [0x77, b'J', b'I', b'F'];

/// JIF file representation
///
pub struct JifFile {
    pheaders: Vec<JifPheader>,
    strings_backing: Vec<u8>,
    itree_nodes: Vec<ITreeNode>,
    ord_chunks: Vec<OrdChunk>,
    data_offset: u64,
    data_segments: Vec<u8>,
}

impl std::fmt::Debug for JifFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let strings = self.strings();
        f.debug_struct("JifFile")
            //.field("pheaders", &self.pheaders)
            //.field("strings", &strings)
            .field("itrees", &self.itree_nodes)
            //.field("ord", &self.ord_chunks)
            .field("data_offset", &self.data_offset)
            .finish()
    }
}

struct JifHeader {
    n_pheaders: u32,
    strings_size: u32,
    itrees_size: u32,
    ord_size: u32,
}

fn read_u8<R: Read>(r: &mut R, buffer: &mut [u8; 1]) -> JifResult<u8> {
    r.read_exact(buffer)?;
    Ok(buffer[0])
}

fn read_u32<R: Read>(r: &mut R, buffer: &mut [u8; 4]) -> JifResult<u32> {
    r.read_exact(buffer)?;
    Ok(u32::from_le_bytes(*buffer))
}

fn read_u64<R: Read>(r: &mut R, buffer: &mut [u8; 8]) -> JifResult<u64> {
    r.read_exact(buffer)?;
    Ok(u64::from_le_bytes(*buffer))
}

/// seek to the next aligned position
/// return the new position
fn seek_to_alignment<R: Read + Seek, const ALIGNMENT: u64>(r: &mut BufReader<R>) -> JifResult<u64> {
    let cur = r.stream_position()?;
    let delta = cur % ALIGNMENT;
    if delta != 0 {
        r.seek_relative((ALIGNMENT - delta) as i64)?;
    }

    Ok(cur + ALIGNMENT - delta)
}

/// seek to the next aligned page
/// return the new position
fn seek_to_page<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<u64> {
    seek_to_alignment::<R, 0x1000>(r)
}

const fn is_aligned<const ALIGNMENT: u64>(v: u64) -> bool {
    v % ALIGNMENT == 0
}

const fn is_page_aligned(v: u64) -> bool {
    is_aligned::<0x1000>(v)
}

impl JifHeader {
    pub fn from_reader<R: Read>(r: &mut R) -> JifResult<Self> {
        let mut buffer = [0u8; 4];
        r.read_exact(&mut buffer)?;

        if buffer != JIF_MAGIC_HEADER {
            return Err(JifError::BadMagic);
        }

        let n_pheaders = read_u32(r, &mut buffer)?;
        let strings_size = read_u32(r, &mut buffer)?;
        if (!is_page_aligned(strings_size as u64)) {
            return Err(JifError::BadAlignment);
        }
        let itrees_size = read_u32(r, &mut buffer)?;
        if (!is_page_aligned(itrees_size as u64)) {
            return Err(JifError::BadAlignment);
        }
        let ord_size = read_u32(r, &mut buffer)?;
        if (!is_page_aligned(ord_size as u64)) {
            return Err(JifError::BadAlignment);
        }

        Ok(JifHeader {
            n_pheaders,
            strings_size,
            itrees_size,
            ord_size,
        })
    }
}

impl JifFile {
    pub fn from_reader<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<Self> {
        let header = JifHeader::from_reader(r)?;

        let pheaders = (0..header.n_pheaders)
            .map(|idx| JifPheader::from_reader(r, idx as usize))
            .collect::<Result<Vec<_>, _>>()?;

        let n_itree_nodes = pheaders
            .iter()
            .map(|h| h.itree_n_nodes as usize)
            .sum::<usize>();

        seek_to_page(r)?;

        // read strings
        let strings_backing = {
            let mut s = Vec::with_capacity(header.strings_size as usize);
            let mut string_reader = r.take(header.strings_size as u64);

            string_reader.read_to_end(&mut s)?;
            Ok::<_, JifError>(s)
        }?;

        // read itree nodes
        let to_skip =
            header.itrees_size as i64 - (n_itree_nodes * ITreeNode::serialized_size()) as i64;
        let itree_nodes = (0..n_itree_nodes)
            .map(|idx| ITreeNode::from_reader(r, idx as usize))
            .collect::<Result<Vec<_>, _>>()?;
        r.seek_relative(to_skip)?;

        let n_ords = header.ord_size as usize / OrdChunk::serialized_size();
        let ord_chunks = (0..n_ords)
            .map(|_| OrdChunk::from_reader(r))
            .collect::<Result<Vec<_>, _>>()?;

        let data_offset = seek_to_page(r)?;
        let data_segments = {
            let mut ds = Vec::new();
            r.read_to_end(&mut ds)?;

            Ok::<_, JifError>(ds)
        }?;

        Ok(JifFile {
            pheaders,
            strings_backing,
            itree_nodes,
            ord_chunks,
            data_offset,
            data_segments,
        })
    }

    pub fn strings(&self) -> Vec<&str> {
        let first_last_zero = self
            .strings_backing
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| **c != 0u8)
            .map(|(idx, _)| std::cmp::min(idx + 1, self.strings_backing.len()))
            .unwrap_or(self.strings_backing.len());

        eprintln!("{}", first_last_zero);

        self.strings_backing[..first_last_zero]
            .split(|x| *x == 0)
            .map(|s| from_utf8(s).unwrap_or("<failed to parse>"))
            .collect::<Vec<&str>>()
    }
}

#[derive(Debug)]
pub struct JifPheader {
    vbegin: u64,
    vend: u64,

    data_begin: u64,
    data_end: u64,

    ref_begin: u64,
    ref_end: u64,

    itree_idx: u32,
    itree_n_nodes: u32,

    pathname_idx: u32,

    prot: u8,
}

impl JifPheader {
    const fn serialized_size() -> usize {
        6 * std::mem::size_of::<u64>() + 3 * std::mem::size_of::<u32>() + std::mem::size_of::<u8>()
    }

    pub fn from_reader<R: Read>(r: &mut R, pheader_idx: usize) -> JifResult<Self> {
        fn read_page_aligned_u64<R: Read>(
            r: &mut R,
            buffer: &mut [u8; 8],
            special_value: bool,
            pheader_idx: usize,
        ) -> JifResult<u64> {
            let v = read_u64(r, buffer)?;

            // MAX is a special value
            if special_value && v == u64::MAX {
                return Ok(v);
            }

            if (!is_page_aligned(v)) {
                Err(JifError::BadPheader {
                    pheader_idx,
                    pheader_err: PheaderError::BadAlignment(v),
                })
            } else {
                Ok(v)
            }
        }
        fn read_page_aligned_u64_pair<R: Read>(
            r: &mut R,
            buffer: &mut [u8; 8],
            special_value: bool,
            pheader_idx: usize,
        ) -> JifResult<(u64, u64)> {
            let begin = read_page_aligned_u64(r, buffer, special_value, pheader_idx)?;
            let end = read_page_aligned_u64(r, buffer, special_value, pheader_idx)?;

            if special_value {
                if begin == u64::MAX && end == u64::MAX {
                    return Ok((begin, end));
                } else if begin == u64::MAX {
                    return Err(JifError::BadPheader {
                        pheader_idx,
                        pheader_err: PheaderError::BadAlignment(begin),
                    });
                } else if end == u64::MAX {
                    return Err(JifError::BadPheader {
                        pheader_idx,
                        pheader_err: PheaderError::BadAlignment(end),
                    });
                }
            }

            if (begin >= end) {
                Err(JifError::BadPheader {
                    pheader_idx,
                    pheader_err: PheaderError::BadRange(begin, end),
                })
            } else {
                Ok((begin, end))
            }
        }

        let mut buffer_8 = [0u8; 8];
        let (vbegin, vend) =
            read_page_aligned_u64_pair(r, &mut buffer_8, false /* special */, pheader_idx)?;
        let (data_begin, data_end) =
            read_page_aligned_u64_pair(r, &mut buffer_8, false /* special */, pheader_idx)?;
        let (ref_begin, ref_end) =
            read_page_aligned_u64_pair(r, &mut buffer_8, true /* special */, pheader_idx)?;

        let mut buffer_4 = [0u8; 4];
        let itree_idx = read_u32(r, &mut buffer_4)?;
        let itree_n_nodes = read_u32(r, &mut buffer_4)?;

        let pathname_idx = read_u32(r, &mut buffer_4)?;

        if ref_begin == u64::MAX && pathname_idx != u32::MAX
            || ref_begin != u64::MAX && pathname_idx == u32::MAX
        {
            return Err(JifError::BadPheader {
                pheader_idx,
                pheader_err: PheaderError::BadRefRange {
                    begin: ref_begin,
                    end: ref_end,
                    pathname_offset: pathname_idx,
                },
            });
        }

        let mut buffer_1 = [0u8; 1];
        let prot = read_u8(r, &mut buffer_1)?;

        Ok(JifPheader {
            vbegin,
            vend,
            data_begin,
            data_end,
            ref_begin,
            ref_end,
            itree_idx,
            itree_n_nodes,
            pathname_idx,
            prot,
        })
    }
}

#[derive(Default, Clone, Copy)]
struct Interval {
    start: u64,
    end: u64,
    offset: u64,
}
impl Interval {
    const fn serialized_size() -> usize {
        3 * std::mem::size_of::<u64>()
    }

    fn valid(&self) -> bool {
        self.start != u64::MAX && self.end != u64::MAX && self.offset != u64::MAX
    }

    pub fn from_reader<R: Read>(
        r: &mut R,
        itree_node_idx: usize,
        interval_idx: usize,
    ) -> JifResult<Self> {
        fn read_page_aligned_u64<R: Read>(
            r: &mut R,
            buffer: &mut [u8; 8],
            itree_node_idx: usize,
            interval_idx: usize,
        ) -> JifResult<u64> {
            let v = read_u64(r, buffer)?;

            // MAX is a special value
            if v == u64::MAX {
                return Ok(v);
            }

            if (!is_page_aligned(v)) {
                Err(JifError::BadITreeNode {
                    itree_node_idx,
                    itree_node_err: ITreeNodeError {
                        interval_idx,
                        interval_err: IntervalError::BadAlignment(v),
                    },
                })
            } else {
                Ok(v)
            }
        }

        let mut buffer = [0u8; 8];

        let start = read_page_aligned_u64(r, &mut buffer, itree_node_idx, interval_idx)?;
        let end = read_page_aligned_u64(r, &mut buffer, itree_node_idx, interval_idx)?;
        let offset = read_page_aligned_u64(r, &mut buffer, itree_node_idx, interval_idx)?;

        if (start == u64::MAX || end == u64::MAX || offset == u64::MAX)
            && (start != end || end != offset)
        {
            return Err(JifError::BadITreeNode {
                itree_node_idx,
                itree_node_err: ITreeNodeError {
                    interval_idx,
                    interval_err: IntervalError::InvalidInterval(start, end, offset),
                },
            });
        }

        if (start >= end) {
            return Err(JifError::BadITreeNode {
                itree_node_idx,
                itree_node_err: ITreeNodeError {
                    interval_idx,
                    interval_err: IntervalError::BadRange(start, end),
                },
            });
        }

        Ok(Interval { start, end, offset })
    }
}

impl std::fmt::Debug for Interval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.valid() {
            f.debug_struct("EmptyInterval").finish()
        } else {
            f.debug_struct("Interval")
                .field("start", &self.start)
                .field("end", &self.end)
                .field("offset", &self.offset)
                .finish()
        }
    }
}

#[derive(Debug)]
struct ITreeNode {
    ranges: [Interval; IVAL_PER_NODE],
}
impl ITreeNode {
    const fn serialized_size() -> usize {
        IVAL_PER_NODE * Interval::serialized_size()
    }

    pub fn from_reader<R: Read>(r: &mut R, itree_node_idx: usize) -> JifResult<Self> {
        let mut ranges = [Interval::default(); IVAL_PER_NODE];
        for idx in 0..IVAL_PER_NODE {
            // TODO: remove unwrap or else
            ranges[idx] = Interval::from_reader(r, itree_node_idx, idx).unwrap_or(Interval {
                start: u64::MAX,
                end: u64::MAX,
                offset: u64::MAX,
            });
        }

        Ok(ITreeNode { ranges })
    }
}

#[derive(Debug)]
struct OrdChunk {
    /// first 42 bits encode the page number of the first page
    vaddr: u64,

    /// last 12 bits encode the number of pages
    n_pages: u16,
}

impl OrdChunk {
    const fn serialized_size() -> usize {
        std::mem::size_of::<u64>()
    }
    pub fn from_reader<R: Read>(r: &mut R) -> JifResult<Self> {
        let mut buffer = [0u8; 8];
        let vaddr_and_n_pages = read_u64(r, &mut buffer)?;
        let vaddr = vaddr_and_n_pages & !0xfff;
        let n_pages = vaddr_and_n_pages as u16 & 0xfff;
        Ok(OrdChunk { vaddr, n_pages })
    }
}
