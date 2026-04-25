#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosixClass {
    Alnum,
    Alpha,
    Blank,
    Cntrl,
    Digit,
    Graph,
    Lower,
    Print,
    Punct,
    Space,
    Upper,
    XDigit,
}

impl PosixClass {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "alnum" => Some(Self::Alnum),
            "alpha" => Some(Self::Alpha),
            "blank" => Some(Self::Blank),
            "cntrl" => Some(Self::Cntrl),
            "digit" => Some(Self::Digit),
            "graph" => Some(Self::Graph),
            "lower" => Some(Self::Lower),
            "print" => Some(Self::Print),
            "punct" => Some(Self::Punct),
            "space" => Some(Self::Space),
            "upper" => Some(Self::Upper),
            "xdigit" => Some(Self::XDigit),
            _ => None,
        }
    }
}

pub fn class_contains(class: PosixClass, ch: char) -> bool {
    match class {
        PosixClass::Alnum => {
            class_contains(PosixClass::Alpha, ch) || class_contains(PosixClass::Digit, ch)
        }
        PosixClass::Alpha => ch.is_alphabetic(),
        PosixClass::Blank => matches!(ch, ' ' | '\t'),
        PosixClass::Cntrl => ch.is_control(),
        PosixClass::Digit => ch.is_ascii_digit(),
        PosixClass::Graph => !ch.is_control() && !ch.is_whitespace(),
        PosixClass::Lower => ch.is_lowercase(),
        PosixClass::Print => !ch.is_control(),
        PosixClass::Punct => {
            class_contains(PosixClass::Graph, ch) && !class_contains(PosixClass::Alnum, ch)
        }
        PosixClass::Space => matches!(ch, ' ' | '\t' | '\r' | '\n' | '\x0C' | '\x0B'),
        PosixClass::Upper => ch.is_uppercase(),
        PosixClass::XDigit => ch.is_ascii_hexdigit(),
    }
}
