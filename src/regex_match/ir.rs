#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GnuRegex {
    pub expr: GnuExpr,
    pub capture_count: u16,
}

impl GnuRegex {
    pub fn capture_count(&self) -> u16 {
        self.capture_count
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GnuExpr {
    Empty,
    Literal(u8),
    Dot,
    Concat(Vec<GnuExpr>),
    Alternation(Vec<GnuExpr>),
    Group {
        capture_index: u16,
        expr: Box<GnuExpr>,
    },
    Class(ClassExpr),
    Repeat {
        expr: Box<GnuExpr>,
        kind: RepetitionKind,
    },
    Anchor(AnchorKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassExpr {
    pub negated: bool,
    pub items: Vec<ClassItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassItem {
    Byte(u8),
    PosixClass(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepetitionKind {
    ZeroOrMore,
    OneOrMore,
    ZeroOrOne,
    Bounded { min: u32, max: Option<u32> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorKind {
    Start,
    End,
}
