use rushfind::file_flags::{FileFlagsMatcher, FlagCondition, FlagMatchMode};

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
