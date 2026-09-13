use rushfind::file_flags::{
    FileFlagsMatcher, FlagCondition, FlagMatchMode, FlagSpec, parse_flags_argument,
    parse_flags_argument_with_mask,
};
use std::ffi::OsStr;

#[test]
fn flags_exact_all_and_any_use_the_shared_no_prefix_algebra() {
    let exact = FileFlagsMatcher::new(
        FlagMatchMode::Exact,
        0b11,
        vec![FlagCondition::set(0b01), FlagCondition::clear(0b10)],
    );
    let all = FileFlagsMatcher::new(
        FlagMatchMode::All,
        0b11,
        vec![FlagCondition::set(0b01), FlagCondition::clear(0b10)],
    );
    let any = FileFlagsMatcher::new(
        FlagMatchMode::Any,
        0b11,
        vec![FlagCondition::set(0b10), FlagCondition::clear(0b10)],
    );

    assert!(exact.matches(Some(0b01)));
    assert!(all.matches(Some(0b01)));
    assert!(any.matches(Some(0b01)));
}

#[test]
fn flags_are_false_when_flag_bits_are_unknown() {
    let expr = FileFlagsMatcher::new(FlagMatchMode::All, 0b01, vec![FlagCondition::set(0b01)]);

    assert!(!expr.matches(None));
}

#[test]
fn exact_contradictory_conditions_are_false() {
    let matcher = FileFlagsMatcher::new(
        FlagMatchMode::Exact,
        0b1,
        vec![FlagCondition::set(0b1), FlagCondition::clear(0b1)],
    );
    assert!(!matcher.matches(Some(0)));
    assert!(!matcher.matches(Some(1)));
}

#[test]
fn dump_and_nonodump_are_shared_nodump_clear_forms() {
    static SPECS: &[FlagSpec] = &[FlagSpec {
        name: "nodump",
        bit: 1,
    }];

    let dump = parse_flags_argument(OsStr::new("dump"), SPECS).unwrap();
    let nonodump = parse_flags_argument(OsStr::new("nonodump"), SPECS).unwrap();
    assert_eq!(dump, nonodump);
    assert!(dump.matches(Some(0)));
    assert!(!dump.matches(Some(1)));
}

#[test]
fn exact_matching_includes_unnamed_active_bits() {
    static SPECS: &[FlagSpec] = &[FlagSpec {
        name: "arch",
        bit: 1,
    }];
    let matcher = parse_flags_argument_with_mask(OsStr::new("arch"), SPECS, 0b11).unwrap();
    assert!(matcher.matches(Some(0b01)));
    assert!(!matcher.matches(Some(0b11)));
}
