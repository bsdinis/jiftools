# TODO

## Fixable Errors

- Add a notion of fixable errors: will be propagated up and returned (optionally) to the user
- API idea: `from_reader` will return Err(e) for all errors; `from_reader_permissible` will return `Err(e)` on irrecoverable errors but `Ok((Jif, E))` for fixable ones

- Examples of fixable errors:
    - Pheaders not sorted
    - Bad ordering segments (more than one interval)

- The notion of fixable errors means that these errors should not be caught in the parsing of the file, but delayed to the validation portion
- True parsing is not entirely possible/ergonomic because
    1. We do want to expose these recoverable errors on occasion
    2. Sometimes the errors require data that is not locally present while parsing
