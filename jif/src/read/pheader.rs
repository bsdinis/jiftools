use crate::error::*;
use crate::pheader::JifRawPheader;
use crate::utils::{is_page_aligned, read_u32, read_u64, read_u8};

use std::io::Read;

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

    if !is_page_aligned(v) {
        Err(JifError::BadPheader {
            pheader_idx,
            pheader_err: PheaderError::BadAlignment(v),
        })
    } else {
        Ok(v)
    }
}

fn read_page_aligned_u64_pair<R: Read, F: FnOnce(u64, u64) -> PheaderError>(
    r: &mut R,
    buffer: &mut [u8; 8],
    pheader_idx: usize,
    pheader_error_builder: F,
) -> JifResult<(u64, u64)> {
    let begin = read_page_aligned_u64(r, buffer, false /* special value */, pheader_idx)?;
    let end = read_page_aligned_u64(r, buffer, false /* special value */, pheader_idx)?;

    if begin > end {
        Err(JifError::BadPheader {
            pheader_idx,
            pheader_err: pheader_error_builder(begin, end),
        })
    } else {
        Ok((begin, end))
    }
}

fn read_virtual_range<R: Read>(
    r: &mut R,
    buffer: &mut [u8; 8],
    pheader_idx: usize,
) -> JifResult<(u64, u64)> {
    read_page_aligned_u64_pair(r, buffer, pheader_idx, PheaderError::BadVirtualRange)
}

fn read_ref_offset<R: Read>(r: &mut R, buffer: &mut [u8; 8], pheader_idx: usize) -> JifResult<u64> {
    read_page_aligned_u64(r, buffer, true /* special value */, pheader_idx)
}

impl JifRawPheader {
    /// Read and parse a pheader
    pub fn from_reader<R: Read>(r: &mut R, pheader_idx: usize) -> JifResult<Self> {
        let mut buffer_8 = [0u8; 8];
        let (vbegin, vend) = read_virtual_range(r, &mut buffer_8, pheader_idx)?;
        let ref_offset = read_ref_offset(r, &mut buffer_8, pheader_idx)?;

        let mut buffer_4 = [0u8; 4];
        let itree_idx = read_u32(r, &mut buffer_4)?;
        let itree_n_nodes = read_u32(r, &mut buffer_4)?;

        let pathname_offset = read_u32(r, &mut buffer_4)?;

        if ref_offset == u64::MAX && pathname_offset != u32::MAX {
            return Err(JifError::BadPheader {
                pheader_idx,
                pheader_err: PheaderError::BadRefRange {
                    offset: ref_offset,
                    pathname_offset,
                },
            });
        }

        let mut buffer_1 = [0u8; 1];
        let prot = read_u8(r, &mut buffer_1)?;

        Ok(JifRawPheader {
            vbegin,
            vend,
            ref_offset,
            itree_idx,
            itree_n_nodes,
            pathname_offset,
            prot,
        })
    }
}
