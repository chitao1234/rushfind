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
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    And(Vec<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Sequence(Vec<Expr>),
    Not(Box<Expr>),
    Predicate(Predicate),
    Action(Action),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Predicate {
    MaxDepth(u32),
    MinDepth(u32),
    Depth,
    Prune,
    XDev,
    Readable,
    Writable,
    Executable,
    Name {
        pattern: OsString,
        case_insensitive: bool,
    },
    Path {
        pattern: OsString,
        case_insensitive: bool,
    },
    Regex {
        pattern: OsString,
        case_insensitive: bool,
    },
    RegexType(OsString),
    FsType(OsString),
    Inum(OsString),
    Links(OsString),
    SameFile(PathBuf),
    LName {
        pattern: OsString,
        case_insensitive: bool,
    },
    Uid(OsString),
    Gid(OsString),
    User(OsString),
    Group(OsString),
    Owner(OsString),
    OwnerSid(OsString),
    GroupSid(OsString),
    NoUser,
    NoGroup,
    Perm(OsString),
    Flags(OsString),
    ReparseType(OsString),
    Size(OsString),
    Empty,
    Used(OsString),
    ATime(OsString),
    CTime(OsString),
    MTime(OsString),
    AMin(OsString),
    CMin(OsString),
    MMin(OsString),
    Newer(PathBuf),
    ANewer(PathBuf),
    CNewer(PathBuf),
    NewerXY {
        current: char,
        reference: char,
        reference_arg: OsString,
    },
    DayStart,
    Type(FileTypeMatcher),
    XType(FileTypeMatcher),
    True,
    False,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Print,
    Print0,
    Printf { format: OsString },
    FPrint { path: PathBuf },
    FPrint0 { path: PathBuf },
    FPrintf { path: PathBuf, format: OsString },
    Ls,
    Fls { path: PathBuf },
    Quit,
    Exec { argv: Vec<OsString>, batch: bool },
    ExecDir { argv: Vec<OsString>, batch: bool },
    Ok { argv: Vec<OsString>, batch: bool },
    OkDir { argv: Vec<OsString>, batch: bool },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileTypeMatcher {
    bits: u8,
}

impl FileTypeMatcher {
    pub fn single(filter: FileTypeFilter) -> Self {
        Self { bits: filter.bit() }
    }

    pub fn from_filters(filters: impl IntoIterator<Item = FileTypeFilter>) -> Self {
        let bits = filters
            .into_iter()
            .fold(0, |bits, filter| bits | filter.bit());
        Self { bits }
    }

    pub fn contains(self, filter: FileTypeFilter) -> bool {
        self.bits & filter.bit() != 0
    }
}

impl From<FileTypeFilter> for FileTypeMatcher {
    fn from(filter: FileTypeFilter) -> Self {
        Self::single(filter)
    }
}

impl FileTypeFilter {
    const fn bit(self) -> u8 {
        match self {
            Self::File => 1 << 0,
            Self::Directory => 1 << 1,
            Self::Symlink => 1 << 2,
            Self::Block => 1 << 3,
            Self::Character => 1 << 4,
            Self::Fifo => 1 << 5,
            Self::Socket => 1 << 6,
        }
    }
}
