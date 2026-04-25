pub fn fold_char(ch: char) -> char {
    match ch {
        'A'..='Z' => ch.to_ascii_lowercase(),
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
