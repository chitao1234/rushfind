pub fn fold_char(ch: char) -> char {
    // The Unicode tables add nothing for ASCII, which is the common case here.
    if ch.is_ascii() {
        return ch.to_ascii_lowercase();
    }

    match ch {
        'À' => 'à',
        'Á' => 'á',
        'Â' => 'â',
        'Ã' => 'ã',
        'Ä' => 'ä',
        'Å' => 'å',
        'Æ' => 'æ',
        'Ç' => 'ç',
        'È' => 'è',
        'É' => 'é',
        'Ê' => 'ê',
        'Ë' => 'ë',
        'Ì' => 'ì',
        'Í' => 'í',
        'Î' => 'î',
        'Ï' => 'ï',
        'Ð' => 'ð',
        'Ñ' => 'ñ',
        'Ò' => 'ò',
        'Ó' => 'ó',
        'Ô' => 'ô',
        'Õ' => 'õ',
        'Ö' => 'ö',
        'Ø' => 'ø',
        'Ù' => 'ù',
        'Ú' => 'ú',
        'Û' => 'û',
        'Ü' => 'ü',
        'Ý' => 'ý',
        'Þ' => 'þ',
        other => {
            let mut lowered = other.to_lowercase();
            match (lowered.next(), lowered.next()) {
                (Some(single), None) => single,
                _ => other,
            }
        }
    }
}

pub fn chars_equal_folded(left: char, right: char) -> bool {
    fold_char(left) == fold_char(right)
}

#[cfg(test)]
mod tests {
    use super::{chars_equal_folded, fold_char};

    #[test]
    fn ascii_folds_without_consulting_the_unicode_tables() {
        for (input, expected) in [
            ('A', 'a'),
            ('z', 'z'),
            ('0', '0'),
            ('/', '/'),
            ('é', 'é'),
            ('É', 'é'),
            ('\u{212a}', 'k'),
        ] {
            assert_eq!(fold_char(input), expected, "{input:?}");
        }

        assert!(chars_equal_folded('K', 'k'));
        assert!(chars_equal_folded('\u{212a}', 'k'));
        assert!(!chars_equal_folded('k', 'g'));
    }
}
