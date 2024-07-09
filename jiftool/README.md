# `jiftool`

A tool for modifying JIF files

## Example usage:
```sh
$ jiftool orig.jif terse.jif # remove duplicate strings, etc.
$ jiftool orig.jif new.jif rename /usr/bin/ld.so /bin/ld.so # rename path to `ld.so`
$ jiftool orig.jif itree.jif build-itrees # build interval trees
$ jiftool orig.jif ordered.jif add-ord tsa.ord # add an ordering section
```

## Usage Reference

### Basic

```
$ jiftool --help
Modify JIF files

Usage: jiftool [OPTIONS] <FILE> <FILE> [COMMAND]

Commands:
  rename        Rename a referenced file in the JIF
  build-itrees  Build the interval trees in the JIF
  add-ord       Add an ordering section
  help          Print this message or the help of the given subcommand(s)

Arguments:
  <FILE>  Input file path
  <FILE>  Output file path

Options:
      --show     Whether to print out the resulting JIF
  -h, --help     Print help
  -V, --version  Print version
```

### Rename

```
$ jiftool help rename
Rename a referenced file in the JIF

Usage: jiftool <FILE> <FILE> rename <FILE> <FILE>

Arguments:
  <FILE>  Old name
  <FILE>  New name

Options:
  -h, --help  Print help
```

### Build Interval Trees

```
$ jiftool help build-itrees
Build the interval trees in the JIF

Usage: jiftool <FILE> <FILE> build-itrees

Options:
  -h, --help  Print help
```

### Adding an Ordering section

```
$ jiftool help add-ord
Add an ordering section

Ingests a timestamped access log (each line of format `<usecs>: <address>`) to construct the ordering list

Usage: jiftool <FILE> <FILE> add-ord [FILE]

Arguments:
  [FILE]
          Filepath of the timestamped access log (defaults to `stdin`)

Options:
  -h, --help
          Print help (see a summary with '-h')
```
