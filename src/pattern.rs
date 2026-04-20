use crate::diagnostics::Diagnostic;
use std::ffi::OsStr;

type PatternMatcher = fn(&OsStr, &OsStr, bool, bool) -> Result<bool, Diagnostic>;

pub fn matches_pattern(
    pattern: &OsStr,
    candidate: &OsStr,
    case_insensitive: bool,
    pathname: bool,
) -> Result<bool, Diagnostic> {
    matches_pattern_with(
        pattern,
        candidate,
        case_insensitive,
        pathname,
        crate::platform::unix::match_pattern,
    )
}

fn matches_pattern_with(
    pattern: &OsStr,
    candidate: &OsStr,
    case_insensitive: bool,
    pathname: bool,
    matcher: PatternMatcher,
) -> Result<bool, Diagnostic> {
    matcher(pattern, candidate, case_insensitive, pathname)
}

#[cfg(test)]
mod tests {
    use super::matches_pattern_with;
    use std::ffi::OsStr;

    #[test]
    fn case_insensitive_matching_can_use_a_non_linux_backend() {
        let matched = matches_pattern_with(
            OsStr::new("*.rs"),
            OsStr::new("MAIN.RS"),
            true,
            true,
            |_pattern, _candidate, case_insensitive, pathname| {
                assert!(case_insensitive);
                assert!(pathname);
                Ok(true)
            },
        )
        .unwrap();

        assert!(matched);
    }
}
