use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAst {
    pub start_paths: Vec<PathBuf>,
    pub expr: Expr,
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
        pattern: String,
        case_insensitive: bool,
    },
    Path {
        pattern: String,
        case_insensitive: bool,
    },
    Type(FileTypeFilter),
    True,
    False,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Print,
    Print0,
    Exec {
        argv: Vec<String>,
        batch: bool,
    },
    ExecDir {
        argv: Vec<String>,
        batch: bool,
    },
    Ok {
        argv: Vec<String>,
    },
    OkDir {
        argv: Vec<String>,
    },
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
