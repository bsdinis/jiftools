# `readjif`

A tool to read and query JIF files

## Sample Usage

Example usage:
```sh
$ readjif a.jif # reads the jif file, dumps a representation of the materialized JIF
$ readjif --raw a.jif # reads the jif file, dumps a representation of the raw JIF
```

Additionally, there is support for selectively querying the JIF.

### Materialized query selectors

For materialized JIFs, the API is the following:
- `jif`: select the whole JIF
- `jif.strings`: strings in the JIF (incompatible with the page selectors)
- `jif.zero_pages`: number of zero pages
- `jif.private_pages`: number of private pages in the JIF
- `jif.shared_pages`: number of shared pages in the pheader
- `jif.pages`: total number of pages
- `ord`: select all the ord chunks
- `ord[<range>]`: select the ord chunks in the range
- `ord.len`: number of ord chunks (incompatible with the range selector)
- `pheader`: select all the pheaders
- `pheader[<range>]`: select the pheaders in the range
- `pheader.len`: number of pheaders (incompatible with the range and field selectors)
- `pheader.data_size`: size of the data region (mixable with range and other selectors)
- `pheader.pathname`: reference pathname (mixable with range and other selectors)
- `pheader.ref_offset`: offset into the file
- `pheader.virtual_range`: virtual address range of the pheader (mixable with range and other selectors)
- `pheader.virtual_size`: size of the virtual address range (mixable with range and other selectors)
- `pheader.prot`: area `rwx` protections (mixable with range and other selectors)
- `pheader.itree`: pheader interval tree (mixable with range and other selectors)
- `pheader.n_itree_nodes`: number of interval tree nodes in pheader (mixable with range and other selectors)
- `pheader.zero_pages`: number of zero pages
- `pheader.private_pages`: the same as `data_size % PAGE_SIZE`
- `pheader.shared_pages`: number of shared pages in the pheader
- `pheader.pages`: total number of pages

### Raw query selectors

For raw JIFs, the API is similar:
- `jif`: select the whole JIF
- `jif.data`: size of the data section
- `jif.zero_pages`: number of zero pages
- `jif.private_pages`: the same as `data % PAGE_SIZE`
- `jif.pages`: total number of pages
- `strings`: select the strings in the JIF
- `itrees`: select all the interval trees
- `itrees[<range>]`: select the interval trees in the range
- `itrees.len`: number of interval trees (incompatible with the range selector)
- `ord`: select all the ord chunks
- `ord[<range>]`: select the ord chunks in the range
- `ord.len`: number of ord chunks (incompatible with the range selector)
- `pheader`: select all the pheaders
- `pheader[<range>]`: select the pheaders in the range
- `pheader.len`: number of pheaders (incompatible with the range and field selectors)
- `pheader.pathname_offset`: reference pathname (mixable with range and other selectors)
- `pheader.ref_offset`: offset into the file
- `pheader.virtual_range`: virtual address range of the pheader (mixable with range and other selectors)
- `pheader.virtual_size`: size of the virtual address range (mixable with range and other selectors)
- `pheader.prot`: area `rwx` protections (mixable with range and other selectors)
- `pheader.itree`: show the interval tree offset and size in number of nodes (mixable with range and other selectors)

## Usage

```
$ readjif --help
readjif: read and query JIF files

Thie tool parses the JIF (optionally materializing it) and allows for querying and viewing the JIF

Usage: readjif [OPTIONS] <FILE> [COMMAND]

Arguments:
  <FILE>
          JIF file to read from

  [COMMAND]
          Selector command

          For help, type `help` as the subcommand

Options:
      --raw
          Use the raw JIF

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```

```
$ readjif file.jif help
Error: failed to parse materialized selector command: unknown selector help
materialized command: selection over the materialized JIF representation

jif                                select the whole JIF
jif.strings                        strings in the JIF
jif.zero_pages                     number of zero pages
jif.private_pages                  number of private pages in the JIF
jif.shared_pages                   number of shared pages in the pheader
jif.pages                          total number of pages

ord                                select all the ord chunks
ord[<range>]                       select the ord chunks in the range
ord.len                            number of ord chunks

pheader                            select all the pheaders
pheader[<range>]                   select the pheaders in the range
pheader.len                        number of pheaders
pheader.data_size                  size of the data region (mixable with range and other selectors)
pheader.pathname                   reference pathname (mixable with range and other selectors)
pheader.ref_offset                 offset into the file
pheader.virtual_range              virtual address range of the pheader (mixable with range and other selectors)
pheader.virtual_size               size of the virtual address range (mixable with range and other selectors)
pheader.prot                       area `rwx` protections (mixable with range and other selectors)
pheader.itree                      pheader interval tree (mixable with range and other selectors)
pheader.n_itree_nodes              number of interval tree nodes in pheader (mixable with range and other selectors)
pheader.zero_pages                 number of zero pages
pheader.private_pages              == data_size % PAGE_SIZE
pheader.shared_pages               number of shared pages in the pheader
pheader.pages                      total number of pages
```

```
$ readjif file.jif --raw help
Error: failed to parse raw selector command: unknown selector help
raw command: selection over the raw JIF representation

jif                                select the whole JIF
jif.data                           size of the data section
jif.zero_pages                     number of zero pages
jif.private_pages                  == data % PAGE_SIZE
jif.pages                          total number of pages

strings                            select the strings in the JIF

itrees                             select all the interval trees
itrees[<range>]                    select the interval trees in the range
itrees.len                         number of interval trees

ord                                select all the ord chunks
ord[<range>]                       select the ord chunks in the range
ord.len                            number of ord chunks

pheader                            select all the pheaders
pheader[<range>]                   select the pheaders in the range
pheader.len                        number of pheaders
pheader.pathname_offset            reference pathname (mixable with range and other selectors)
pheader.ref_offset                 offset into the file
pheader.virtual_range              virtual address range of the pheader (mixable with range and other selectors)
pheader.virtual_size               size of the virtual address range (mixable with range and other selectors)
pheader.prot                       area `rwx` protections (mixable with range and other selectors)
pheader.itree                      show the interval tree offset and size in number of nodes (mixable with range and other selectors)
```
