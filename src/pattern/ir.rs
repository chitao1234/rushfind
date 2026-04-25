use crate::ctype::class::PosixClass;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GlobAtom {
    Literal(u8),
    AnyByte,
    AnySequence,
    Class(GlobClass),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobClass {
    pub negated: bool,
    pub items: Vec<ClassItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassItem {
    Literal(u8),
    Range(u8, u8),
    Posix(PosixClass),
}

pub type GlobProgram = Vec<GlobAtom>;
