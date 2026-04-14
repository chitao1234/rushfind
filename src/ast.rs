use crate::follow::FollowMode;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAst {
    pub start_paths: Vec<PathBuf>,
    pub global_options: Vec<GlobalOption>,
    pub expr: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalOption {
    Follow(FollowMode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    And(Vec<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Predicate(Predicate),
    Action(Action),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    MaxDepth(u32),
    MinDepth(u32),
    Name {
        pattern: OsString,
        case_insensitive: bool,
    },
    Path {
        pattern: OsString,
        case_insensitive: bool,
    },
    Type(FileTypeFilter),
    XType(FileTypeFilter),
    True,
    False,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Print,
    Print0,
    Exec { argv: Vec<OsString>, batch: bool },
    ExecDir { argv: Vec<OsString>, batch: bool },
    Ok { argv: Vec<OsString> },
    OkDir { argv: Vec<OsString> },
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTypeFilter {
    File,
    Directory,
    Symlink,
    Block,
    Character,
    Fifo,
    Socket,
}
