use rushfind::ctype::text::{TextUnit, decode_units};
use rushfind::ctype::{CtypeProfile, resolve_ctype_profile_from};

fn profile(locale: &str) -> CtypeProfile {
    resolve_ctype_profile_from(vec![("LC_CTYPE", locale)])
}

#[test]
fn byte_c_segments_one_byte_per_unit() {
    let profile = CtypeProfile::ByteC;
    let units = decode_units(&profile, b"a\xff/").collect::<Vec<_>>();

    assert_eq!(
        units,
        vec![
            TextUnit::Char {
                ch: 'a',
                bytes: b"a"
            },
            TextUnit::Invalid { bytes: b"\xff" },
            TextUnit::Char {
                ch: '/',
                bytes: b"/"
            },
        ]
    );
}

#[test]
fn utf8_profile_segments_multibyte_scalar_as_one_unit() {
    let profile = profile("en_US.UTF-8");
    let units = decode_units(&profile, "é/".as_bytes()).collect::<Vec<_>>();

    assert_eq!(
        units,
        vec![
            TextUnit::Char {
                ch: 'é',
                bytes: "é".as_bytes()
            },
            TextUnit::Char {
                ch: '/',
                bytes: b"/"
            },
        ]
    );
}

#[test]
fn utf8_profile_preserves_invalid_bytes_as_invalid_units() {
    let profile = profile("en_US.UTF-8");
    let units = decode_units(&profile, b"x\xc3y").collect::<Vec<_>>();

    assert_eq!(
        units,
        vec![
            TextUnit::Char {
                ch: 'x',
                bytes: b"x"
            },
            TextUnit::Invalid { bytes: b"\xc3" },
            TextUnit::Char {
                ch: 'y',
                bytes: b"y"
            },
        ]
    );
}

#[test]
fn shift_jis_profile_segments_multibyte_character() {
    let profile = profile("ja_JP.SJIS");
    let units = decode_units(&profile, b"\x82\xa0z").collect::<Vec<_>>();

    assert_eq!(
        units,
        vec![
            TextUnit::Char {
                ch: 'あ',
                bytes: b"\x82\xa0"
            },
            TextUnit::Char {
                ch: 'z',
                bytes: b"z"
            },
        ]
    );
}
