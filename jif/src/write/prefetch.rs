use crate::prefetch::{PrefetchWindow, WindowingStrategy};
use std::io::Write;

impl WindowingStrategy {
    /// Write an ordering chunk
    pub fn to_writer<W: Write>(&self, w: &mut W) -> std::io::Result<usize> {
        let (strat_id, argument): (u32, u32) = match self {
            WindowingStrategy::NoPrefetch => (0, 0),
            WindowingStrategy::Single => (1, 0),
            WindowingStrategy::UniformTime(argument) => (2, *argument),
            WindowingStrategy::UniformVolume(argument) => (3, *argument),
            WindowingStrategy::LinearTime(argument) => (4, *argument),
            WindowingStrategy::LinearVolume(argument) => (5, *argument),
            WindowingStrategy::ExponentialTime(argument) => (6, *argument),
            WindowingStrategy::ExponentialVolume(argument) => (7, *argument),
        };

        w.write_all(&strat_id.to_le_bytes())?;
        w.write_all(&argument.to_le_bytes())?;
        Ok(Self::serialized_size())
    }
}

impl PrefetchWindow {
    /// Write an ordering chunk
    pub fn to_writer<W: Write>(&self, w: &mut W) -> std::io::Result<usize> {
        w.write_all(&self.n_write.to_le_bytes())?;
        w.write_all(&self.n_total.to_le_bytes())?;
        Ok(Self::serialized_size())
    }
}
