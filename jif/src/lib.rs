//! # `jif`
//!
//! `jif` is a library for parsing, dumping and manipulating JIF (Junction Image Format) files

pub mod error;
mod itree;
mod itree_node;
mod jif;
mod ord;
mod pheader;
mod utils;

mod read;
mod write;

pub use itree::*;
pub use jif::*;
pub use ord::*;
pub use pheader::*;

pub use error::{JifError, JifResult};
