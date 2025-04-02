//! The prefetch windows

#[derive(PartialEq, Eq, Copy, Clone, Default)]
pub enum WindowingStrategy {
    #[default]
    /// Don't prefetch
    NoPrefetch,

    /// Create a single prefetch window
    Single,

    /// Create N prefetch windows, based on time
    UniformTime(u32),

    /// Create N prefetch windows, based on volume (# pages)
    UniformVolume(u32),

    /// Linear time windowing (each window corresponds to a time window with constant range)
    /// Time is measured in microseconds
    ///
    /// i.e., LinearTime(x) => window i corresponds to t < (i + 1) * x
    LinearTime(u32),

    /// Linear volume windowing (each window corresponds to a volume window with constant range)
    /// Volume is measured in number of pages
    ///
    /// i.e., LinearVolume(x) => window i corresponds to #n_pages < (i + 1) * x
    LinearVolume(u32),

    /// Exponential time windowing (each window has corresponds to a time window with exponential
    /// range)
    /// Time is measured in microseconds
    ///
    /// i.e., ExponentialTime(x) => window i corresponds to t < (x^(i + 1))
    ExponentialTime(u32),

    /// Exponential volume windowing (each window has corresponds to a volume window with exponential
    /// range)
    /// Volume is measured in number of pages
    ///
    /// i.e., ExponentialTime(x) => window i corresponds to t < (x^(i + 1))
    ExponentialVolume(u32),
}

impl WindowingStrategy {
    /// The size of the [`WindowingStrategy`] when serialized on disk
    pub(crate) const fn serialized_size() -> usize {
        2 * std::mem::size_of::<u32>()
    }
}

/// Window to prefetch.
#[derive(PartialEq, Eq, Copy, Clone)]
pub struct PrefetchWindow {
    /// Number of pages in the window that are written to
    pub(crate) n_write: u64,

    /// Total size (in #pages) of the prefetch window
    pub(crate) n_total: u64,
}

impl PrefetchWindow {
    /// The size of the [`PrefetchWindow`] when serialized on disk
    pub(crate) const fn serialized_size() -> usize {
        2 * std::mem::size_of::<u64>()
    }

    /// Whether this is an empty  PrefetchWindow or not (i.e., no pages)
    pub(crate) fn is_empty(&self) -> bool {
        self.n_write == 0 && self.n_total == 0
    }
}

impl std::fmt::Debug for WindowingStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowingStrategy::NoPrefetch => f.write_str("NoPrefetch"),
            WindowingStrategy::Single => f.write_str("SingleWindow"),
            WindowingStrategy::UniformTime(arg) => {
                f.write_fmt(format_args!("UniformTime(n={arg})"))
            }
            WindowingStrategy::UniformVolume(arg) => {
                f.write_fmt(format_args!("UniformTime(n={arg})"))
            }
            WindowingStrategy::LinearTime(arg) => f.write_fmt(format_args!("LinearTime({arg}us)")),
            WindowingStrategy::LinearVolume(arg) => {
                f.write_fmt(format_args!("LinearVolume({arg} pages)"))
            }
            WindowingStrategy::ExponentialTime(arg) => {
                f.write_fmt(format_args!("ExponentialTime({arg} * 2^t us)"))
            }
            WindowingStrategy::ExponentialVolume(arg) => {
                f.write_fmt(format_args!("ExponentialVolume({arg}us * 2^t us)"))
            }
        }
    }
}

impl std::fmt::Debug for PrefetchWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prefetch")
            .field("n_write", &self.n_write)
            .field("n_total", &self.n_total)
            .finish()
    }
}
