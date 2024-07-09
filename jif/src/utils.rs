use crate::error::*;

use std::io::{BufReader, Read, Seek};

pub(crate) const PAGE_SIZE: usize = 0x1000;

pub(crate) fn read_u8<R: Read>(r: &mut R, buffer: &mut [u8; 1]) -> JifResult<u8> {
    r.read_exact(buffer)?;
    Ok(buffer[0])
}

pub(crate) fn read_u32<R: Read>(r: &mut R, buffer: &mut [u8; 4]) -> JifResult<u32> {
    r.read_exact(buffer)?;
    Ok(u32::from_le_bytes(*buffer))
}

pub(crate) fn read_u64<R: Read>(r: &mut R, buffer: &mut [u8; 8]) -> JifResult<u64> {
    r.read_exact(buffer)?;
    Ok(u64::from_le_bytes(*buffer))
}

/// seek to the next aligned position
/// return the new position
pub(crate) fn seek_to_alignment<R: Read + Seek, const ALIGNMENT: usize>(
    r: &mut BufReader<R>,
) -> JifResult<u64> {
    let cur = r.stream_position()?;
    let delta = cur % ALIGNMENT as u64;
    if delta != 0 {
        r.seek_relative((ALIGNMENT as u64 - delta) as i64)?;
        Ok(cur + ALIGNMENT as u64 - delta)
    } else {
        Ok(cur)
    }
}

/// seek to the next aligned page
/// return the new position
pub(crate) fn seek_to_page<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<u64> {
    seek_to_alignment::<R, PAGE_SIZE>(r)
}

pub(crate) const fn is_aligned<const ALIGNMENT: usize>(v: u64) -> bool {
    v % ALIGNMENT as u64 == 0
}

pub(crate) const fn is_page_aligned(v: u64) -> bool {
    is_aligned::<PAGE_SIZE>(v)
}

pub(crate) const fn align<const ALIGNMENT: usize>(val: u64) -> u64 {
    let delta = val % ALIGNMENT as u64;
    if delta != 0 {
        val + ALIGNMENT as u64 - delta
    } else {
        val
    }
}

pub(crate) const fn page_align(val: u64) -> u64 {
    align::<PAGE_SIZE>(val)
}

pub(crate) const fn align_down<const ALIGNMENT: usize>(val: u64) -> u64 {
    let delta = val % ALIGNMENT as u64;
    if delta != 0 {
        val - delta
    } else {
        val
    }
}

pub(crate) const fn page_align_down(val: u64) -> u64 {
    align_down::<PAGE_SIZE>(val)
}

#[derive(Debug)]
pub(crate) enum PageCmp {
    Same,
    Diff,
    Zero,
}

// ASSUMPTION: page.len() == PAGE_SIZE
pub(crate) fn is_zero(page: &[u8]) -> bool {
    !(0..page.len())
        .step_by(std::mem::size_of::<u128>())
        .map(|x| {
            u128::from_le_bytes(
                page[x..(x + std::mem::size_of::<u128>())]
                    .try_into()
                    .unwrap(),
            )
        })
        .any(|x| x != 0)
}

// ASSUMPTION: base.len() == overlay.len() == PAGE_SIZE
// TODO(array_chunks): waiting on the `array_chunks` (#![feature(iter_array_chunks)]) that carries
// the size information to change the input types to &[u8; PAGE_SIZE]
//
// required right now because the slices are created by `[u8]::chunks_exact`, which does not carry
// size information
pub(crate) fn compare_pages(base: &[u8], overlay: &[u8]) -> PageCmp {
    if is_zero(overlay) {
        return PageCmp::Zero;
    }

    if base == overlay {
        PageCmp::Same
    } else {
        PageCmp::Diff
    }
}
