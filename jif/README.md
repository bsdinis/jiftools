# `jif`

A Rust crate for parsing, dumping and manipulating JIF (Junction Image Format) files.

The library is organized as follows:

 - The types that model JIFs are in `src/{jif,ord,pheader}.rs` and in [`src/itree`](src/itree).
 - The deduper (that coallesces equal segments of memory) is in `src/deduper.rs`.
 - Error types are in [`src/error`](src/error)
 - Utilities are in `src/util.rs`
 - The [`read`](src/read) directory contains all the parsing functionality
 - The [`write`](src/write) directory contains all the dumping functionality

We maintain this _materialized_ vs. _raw_ distinction and parallel across the crate.
A _raw_ type is one that maps very faithfully to the wire format.
A _materialized_ type is one that contains the concept with all the references resolved (e.g., a pathname offset becomes the actual pathname).
