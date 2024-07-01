use crate::error::*;

use std::io::{BufReader, Read, Seek};

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
pub(crate) fn seek_to_alignment<R: Read + Seek, const ALIGNMENT: u64>(
    r: &mut BufReader<R>,
) -> JifResult<u64> {
    let cur = r.stream_position()?;
    let delta = cur % ALIGNMENT;
    if delta != 0 {
        r.seek_relative((ALIGNMENT - delta) as i64)?;
        Ok(cur + ALIGNMENT - delta)
    } else {
        Ok(cur)
    }
}

/// seek to the next aligned page
/// return the new position
pub(crate) fn seek_to_page<R: Read + Seek>(r: &mut BufReader<R>) -> JifResult<u64> {
    seek_to_alignment::<R, 0x1000>(r)
}

pub(crate) const fn is_aligned<const ALIGNMENT: u64>(v: u64) -> bool {
    v % ALIGNMENT == 0
}

pub(crate) const fn is_page_aligned(v: u64) -> bool {
    is_aligned::<0x1000>(v)
}

pub(crate) const fn align<const ALIGNMENT: u64>(val: u64) -> u64 {
    let delta = val % ALIGNMENT;
    if delta != 0 {
        val + ALIGNMENT - delta
    } else {
        val
    }
}

pub(crate) const fn page_align(val: u64) -> u64 {
    align::<0x1000>(val)
}
