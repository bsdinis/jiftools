use crate::error::*;
use crate::prefetch::{PrefetchWindow, WindowingStrategy};
use crate::utils::{read_u32, read_u64};
use std::io::Read;

impl WindowingStrategy {
    /// Read and parse an PrefetchWindow
    pub fn from_reader<R: Read>(r: &mut R) -> WindowingStrategyResult<Self> {
        let mut buffer = [0u8; 4];
        let strat_id = read_u32(r, &mut buffer)?;
        let argument = read_u32(r, &mut buffer)?;

        match strat_id {
            0 => Ok(WindowingStrategy::NoPrefetch),
            1 => Ok(WindowingStrategy::Single),
            2 => Ok(WindowingStrategy::UniformTime(argument)),
            3 => Ok(WindowingStrategy::UniformVolume(argument)),
            4 => Ok(WindowingStrategy::LinearTime(argument)),
            5 => Ok(WindowingStrategy::LinearVolume(argument)),
            6 => Ok(WindowingStrategy::ExponentialTime(argument)),
            7 => Ok(WindowingStrategy::ExponentialVolume(argument)),
            _ => Err(WindowingStrategyError::InvalidId(strat_id)),
        }
    }
}

impl PrefetchWindow {
    /// Read and parse an PrefetchWindow
    pub fn from_reader<R: Read>(r: &mut R) -> PrefetchWindowResult<Self> {
        let mut buffer = [0u8; 8];
        let n_write = read_u64(r, &mut buffer)?;
        let n_total = read_u64(r, &mut buffer)?;

        if n_write > n_total {
            Err(PrefetchWindowError::InvalidFraction {
                write: n_write,
                total: n_total,
            })
        } else {
            Ok(PrefetchWindow { n_write, n_total })
        }
    }
}
