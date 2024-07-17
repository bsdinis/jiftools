//! # `jif`
//!
//! `jif` is a library for parsing, dumping and manipulating JIF (Junction Image Format) files

pub mod deduper;
mod diff;
pub mod error;
pub mod interval;
pub mod itree;
pub mod itree_node;
mod jif;
pub mod ord;
pub mod pheader;
mod utils;

mod read;
mod write;

pub use jif::{Jif, JifRaw};
pub use pheader::Prot;

pub use error::{JifError, JifResult};
