use rushfind::ctype::{CtypeProfile, resolve_ctype_profile_from};

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

#[test]
fn ctype_profile_uses_lc_all_before_lc_ctype_before_lang() {
    let profile = resolve_ctype_profile_from(env(&[
        ("LANG", "en_US.UTF-8"),
        ("LC_CTYPE", "ja_JP.eucJP"),
        ("LC_ALL", "C"),
    ]));

    assert_eq!(profile, CtypeProfile::ByteC);
}

#[test]
fn ctype_profile_resolves_utf8_codeset_aliases() {
    let profile = resolve_ctype_profile_from(env(&[("LC_CTYPE", "en_US.utf8")]));

    assert!(profile.is_encoded_label("UTF-8"));
}

#[test]
fn ctype_profile_resolves_common_legacy_aliases() {
    let euc_jp = resolve_ctype_profile_from(env(&[("LC_CTYPE", "ja_JP.eucJP")]));
    let sjis = resolve_ctype_profile_from(env(&[("LC_CTYPE", "ja_JP.SJIS")]));
    let koi8 = resolve_ctype_profile_from(env(&[("LC_CTYPE", "ru_RU.KOI8-R")]));

    assert!(euc_jp.is_encoded_label("EUC-JP"));
    assert!(sjis.is_encoded_label("Shift_JIS"));
    assert!(koi8.is_encoded_label("KOI8-R"));
}

#[test]
fn ctype_profile_records_unknown_non_c_locale() {
    let profile = resolve_ctype_profile_from(env(&[("LC_CTYPE", "zz_ZZ.X-UNKNOWN")]));

    assert!(profile.is_unknown());
    assert!(
        profile
            .warning()
            .unwrap()
            .contains("unsupported LC_CTYPE encoding")
    );
}
