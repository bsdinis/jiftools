use std::collections::HashSet;

/// Finds if a single option follows the prefix on the string
/// Returns the index into options
pub(crate) fn find_single_option(
    original: &str,
    suffix: &str,
    options: &[&str],
) -> anyhow::Result<usize> {
    for (idx, opt) in options.iter().enumerate() {
        if opt == &suffix {
            return Ok(idx);
        }
    }

    Err(anyhow::anyhow!(
        "failed to find option in `{}`: {:?}",
        original,
        options
    ))
}

/// Finds if multiple options are selected
/// Returns indeces into options
pub(crate) fn find_multiple_option(
    original: &str,
    suffix: &str,
    options: &[&str],
) -> anyhow::Result<HashSet<usize>> {
    // sanity: only one empty option please
    if options.iter().filter(|x| x.is_empty()).count() > 1 {
        return Err(anyhow::anyhow!(
            "invalid options: multiple empty patterns: {:?}",
            options
        ));
    }

    // if one of the options is "" that has to be the only options
    // we special case it here
    let empty_idx = options
        .iter()
        .enumerate()
        .find(|(_idx, opt)| opt.is_empty())
        .map(|(idx, _opt)| idx);

    if let Some(eidx) = empty_idx {
        if suffix.is_empty() {
            let mut set = HashSet::new();
            set.insert(eidx);
            return Ok(set);
        }
    }

    let mut found_options = HashSet::new();
    let mut cursor = 0;
    while cursor < suffix.len() {
        let old_cursor = cursor;

        for (idx, opt) in options
            .iter()
            .enumerate()
            .filter(|(_, opt)| !opt.is_empty())
        {
            if suffix[cursor..].starts_with(opt) {
                found_options.insert(idx);
                cursor += opt.len();
                break;
            }
        }

        if cursor == old_cursor {
            return Err(anyhow::anyhow!(
                "unknown option in `{}` (suffix: {}): {:?}",
                original,
                &suffix[cursor..],
                options
            ));
        }
    }

    Ok(found_options)
}

/// Finds if `suffix` starts with a range
/// if the range is [..], counts as no range
/// returns the suffix after the `]` codepoint
///
pub(crate) fn find_range<'a>(
    original: &str,
    suffix: &'a str,
) -> anyhow::Result<(Option<(Option<usize>, Option<usize>)>, &'a str)> {
    if !suffix.starts_with('[') {
        return Ok((None, suffix));
    }

    let suffix = &suffix[1..];

    if let Some((range, suffix)) = suffix.split_once(']') {
        if let Some((start_str, end_str)) = range.split_once("..") {
            let start = if start_str != "" {
                Some(start_str.parse::<usize>().map_err(|e| {
                    anyhow::anyhow!(
                        "failed to parse start of the interval {} ({}): {}",
                        range,
                        start_str,
                        e
                    )
                })?)
            } else {
                None
            };

            let end = if end_str != "" {
                Some(end_str.parse::<usize>().map_err(|e| {
                    anyhow::anyhow!(
                        "failed to parse end of the interval {} ({}): {}",
                        range,
                        end_str,
                        e
                    )
                })?)
            } else {
                None
            };

            let range = if start.is_none() && end.is_none() {
                None
            } else {
                Some((start, end))
            };

            Ok((range, suffix))
        } else {
            Err(anyhow::anyhow!(
                "failed to find range in {}: invalid range {}",
                original,
                range
            ))
        }
    } else {
        Err(anyhow::anyhow!(
            "failed to find range in {}: unmatched bracket",
            original
        ))
    }
}
