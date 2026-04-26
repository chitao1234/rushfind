use crate::args::{Arg, ArgCursor};
use crate::ast::{
    Action, CommandAst, CompatibilityOptions, CompatibilityPredicate, DebugOption, Expr,
    FileTypeFilter, FileTypeMatcher, Files0From, GlobalOption, Predicate, WarningMode,
};
use crate::diagnostics::Diagnostic;
use crate::follow::FollowMode;
use crate::numeric::validate_numeric_argument;
use crate::time::validate_time_argument;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

pub fn parse_command(argv: &[OsString]) -> Result<CommandAst, Diagnostic> {
    let mut global_options = Vec::new();
    let mut compatibility_options = CompatibilityOptions::default();
    let mut index = 0;

    while index < argv.len() {
        let consumed =
            parse_leading_option(argv, index, &mut global_options, &mut compatibility_options)?;
        if consumed == 0 {
            break;
        }
        index += consumed;
    }

    let path_count = argv[index..]
        .iter()
        .position(|arg| is_expression_start(Arg::new(arg.as_os_str())))
        .unwrap_or(argv[index..].len());
    let split_index = index + path_count;
    let start_paths_explicit = path_count > 0;

    let start_paths = if start_paths_explicit {
        argv[index..split_index].iter().map(PathBuf::from).collect()
    } else {
        vec![PathBuf::from(".")]
    };

    let expr = if split_index == argv.len() {
        Expr::Action(Action::Print)
    } else {
        let mut parser = Parser::new(&argv[split_index..], compatibility_options);
        let expr = parser.parse_sequence_expression()?;
        parser.expect_end()?;
        compatibility_options = parser.into_compatibility_options();
        expr
    };

    Ok(CommandAst {
        start_paths,
        start_paths_explicit,
        global_options,
        compatibility_options,
        expr,
    })
}

fn is_expression_start(arg: Arg<'_>) -> bool {
    arg.matches("!")
        || arg.matches("(")
        || arg.matches(")")
        || arg.matches("-a")
        || arg.matches("-and")
        || arg.matches("-o")
        || arg.matches("-or")
        || arg.matches("-not")
        || arg.starts_with_dash()
}

fn parse_global_option(arg: Arg<'_>) -> Option<GlobalOption> {
    if arg.matches("-P") {
        Some(GlobalOption::Follow(FollowMode::Physical))
    } else if arg.matches("-H") {
        Some(GlobalOption::Follow(FollowMode::CommandLineOnly))
    } else if arg.matches("-L") {
        Some(GlobalOption::Follow(FollowMode::Logical))
    } else if arg.matches("-version") || arg.matches("--version") {
        Some(GlobalOption::Version)
    } else if arg.matches("--help") {
        Some(GlobalOption::Help)
    } else {
        None
    }
}

fn parse_leading_option(
    argv: &[OsString],
    index: usize,
    global_options: &mut Vec<GlobalOption>,
    compatibility_options: &mut CompatibilityOptions,
) -> Result<usize, Diagnostic> {
    let arg = Arg::new(argv[index].as_os_str());

    if let Some(option) = parse_global_option(arg) {
        global_options.push(option);
        return Ok(1);
    }

    if let Some(level) = parse_optimizer_option(arg)? {
        compatibility_options.optimizer_level = Some(level);
        return Ok(1);
    }

    if arg.matches("-D") {
        let raw = argv
            .get(index + 1)
            .ok_or_else(|| Diagnostic::parse("missing argument for `-D`"))?;
        parse_debug_options(raw, compatibility_options);
        return Ok(2);
    }

    Ok(0)
}

fn parse_optimizer_option(arg: Arg<'_>) -> Result<Option<u32>, Diagnostic> {
    let bytes = arg.as_os_str().as_encoded_bytes();
    if bytes == b"-O" {
        return Err(Diagnostic::parse(
            "the -O option must be immediately followed by a decimal integer",
        ));
    }

    let Some(rest) = bytes.strip_prefix(b"-O") else {
        return Ok(None);
    };

    if rest.is_empty() || !rest.iter().all(u8::is_ascii_digit) {
        return Err(Diagnostic::parse(
            "please specify a decimal number immediately after -O",
        ));
    }

    let raw = std::str::from_utf8(rest)
        .map_err(|_| Diagnostic::parse("please specify a decimal number immediately after -O"))?;
    let level = raw
        .parse::<u32>()
        .map_err(|_| Diagnostic::parse("invalid -O optimization level"))?;
    Ok(Some(level))
}

fn parse_debug_options(raw: &OsString, compatibility_options: &mut CompatibilityOptions) {
    for component in raw
        .as_os_str()
        .as_encoded_bytes()
        .split(|byte| *byte == b',')
    {
        if let Some(option) = DebugOption::parse(component) {
            compatibility_options.debug_options.push(option);
        } else {
            compatibility_options
                .unknown_debug_options
                .push(OsString::from(
                    String::from_utf8_lossy(component).into_owned(),
                ));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtomKind {
    MaxDepth,
    MinDepth,
    Depth,
    Prune,
    XDev,
    Access(AccessAtom),
    Glob(GlobAtom),
    Regex {
        case_insensitive: bool,
        flag: &'static str,
    },
    RegexType,
    FsType,
    Context,
    LinkGlob {
        case_insensitive: bool,
        flag: &'static str,
    },
    Ownership(OwnershipAtom),
    FlagPredicate(FlagAtom),
    Size,
    Empty,
    Used,
    Identity(IdentityAtom),
    Time(TimeAtom),
    Newer(NewerAtom),
    DayStart,
    Type {
        follow_symlinks: bool,
    },
    Boolean(bool),
    Output(OutputAtom),
    FileOutput(FileOutputAtom),
    Compatibility(CompatibilityAtom),
    Quit,
    Exec(ExecAtom),
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccessAtom {
    Readable,
    Writable,
    Executable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobAtom {
    Name {
        case_insensitive: bool,
        flag: &'static str,
    },
    Path {
        case_insensitive: bool,
        flag: &'static str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnershipAtom {
    Uid,
    Gid,
    User,
    Group,
    Owner,
    OwnerSid,
    GroupSid,
    NoUser,
    NoGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlagAtom {
    Perm,
    Flags,
    ReparseType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentityAtom {
    Inum,
    Links,
    SameFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeAtom {
    ATime,
    CTime,
    MTime,
    AMin,
    CMin,
    MMin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewerAtom {
    Newer,
    ANewer,
    CNewer,
    NewerXY { current: char, reference: char },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputAtom {
    Print,
    Print0,
    Printf,
    Ls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileOutputAtom {
    FPrint,
    FPrint0,
    FPrintf,
    Fls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecAtom {
    Exec,
    ExecDir,
    Ok,
    OkDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompatibilityAtom {
    Files0From,
    Follow,
    NoLeaf,
    Warn,
    NoWarn,
    IgnoreReaddirRace,
    NoIgnoreReaddirRace,
}

fn classify_atom(token: Arg<'_>) -> Option<AtomKind> {
    classify_structural_atom(token)
        .or_else(|| classify_name_regex_atom(token))
        .or_else(|| classify_metadata_atom(token))
        .or_else(|| classify_time_atom(token))
        .or_else(|| classify_compatibility_atom(token))
        .or_else(|| classify_action_atom(token))
}

fn classify_structural_atom(token: Arg<'_>) -> Option<AtomKind> {
    if token.matches("-maxdepth") {
        Some(AtomKind::MaxDepth)
    } else if token.matches("-mindepth") {
        Some(AtomKind::MinDepth)
    } else if token.matches("-depth") {
        Some(AtomKind::Depth)
    } else if token.matches("-prune") {
        Some(AtomKind::Prune)
    } else if token.matches("-xdev") || token.matches("-mount") {
        Some(AtomKind::XDev)
    } else if token.matches("-daystart") {
        Some(AtomKind::DayStart)
    } else if token.matches("-type") {
        Some(AtomKind::Type {
            follow_symlinks: false,
        })
    } else if token.matches("-xtype") {
        Some(AtomKind::Type {
            follow_symlinks: true,
        })
    } else if token.matches("-true") {
        Some(AtomKind::Boolean(true))
    } else if token.matches("-false") {
        Some(AtomKind::Boolean(false))
    } else {
        None
    }
}

fn classify_name_regex_atom(token: Arg<'_>) -> Option<AtomKind> {
    if token.matches("-name") {
        Some(AtomKind::Glob(GlobAtom::Name {
            case_insensitive: false,
            flag: "-name",
        }))
    } else if token.matches("-iname") {
        Some(AtomKind::Glob(GlobAtom::Name {
            case_insensitive: true,
            flag: "-iname",
        }))
    } else if token.matches("-path") {
        Some(AtomKind::Glob(GlobAtom::Path {
            case_insensitive: false,
            flag: "-path",
        }))
    } else if token.matches("-wholename") {
        Some(AtomKind::Glob(GlobAtom::Path {
            case_insensitive: false,
            flag: "-wholename",
        }))
    } else if token.matches("-ipath") {
        Some(AtomKind::Glob(GlobAtom::Path {
            case_insensitive: true,
            flag: "-ipath",
        }))
    } else if token.matches("-iwholename") {
        Some(AtomKind::Glob(GlobAtom::Path {
            case_insensitive: true,
            flag: "-iwholename",
        }))
    } else if token.matches("-regex") {
        Some(AtomKind::Regex {
            case_insensitive: false,
            flag: "-regex",
        })
    } else if token.matches("-iregex") {
        Some(AtomKind::Regex {
            case_insensitive: true,
            flag: "-iregex",
        })
    } else if token.matches("-regextype") {
        Some(AtomKind::RegexType)
    } else if token.matches("-fstype") {
        Some(AtomKind::FsType)
    } else if token.matches("-context") {
        Some(AtomKind::Context)
    } else if token.matches("-lname") {
        Some(AtomKind::LinkGlob {
            case_insensitive: false,
            flag: "-lname",
        })
    } else if token.matches("-ilname") {
        Some(AtomKind::LinkGlob {
            case_insensitive: true,
            flag: "-ilname",
        })
    } else {
        None
    }
}

fn classify_metadata_atom(token: Arg<'_>) -> Option<AtomKind> {
    if token.matches("-readable") {
        Some(AtomKind::Access(AccessAtom::Readable))
    } else if token.matches("-writable") {
        Some(AtomKind::Access(AccessAtom::Writable))
    } else if token.matches("-executable") {
        Some(AtomKind::Access(AccessAtom::Executable))
    } else if token.matches("-uid") {
        Some(AtomKind::Ownership(OwnershipAtom::Uid))
    } else if token.matches("-gid") {
        Some(AtomKind::Ownership(OwnershipAtom::Gid))
    } else if token.matches("-user") {
        Some(AtomKind::Ownership(OwnershipAtom::User))
    } else if token.matches("-group") {
        Some(AtomKind::Ownership(OwnershipAtom::Group))
    } else if token.matches("-owner") {
        Some(AtomKind::Ownership(OwnershipAtom::Owner))
    } else if token.matches("-owner-sid") {
        Some(AtomKind::Ownership(OwnershipAtom::OwnerSid))
    } else if token.matches("-group-sid") {
        Some(AtomKind::Ownership(OwnershipAtom::GroupSid))
    } else if token.matches("-nouser") {
        Some(AtomKind::Ownership(OwnershipAtom::NoUser))
    } else if token.matches("-nogroup") {
        Some(AtomKind::Ownership(OwnershipAtom::NoGroup))
    } else if token.matches("-perm") {
        Some(AtomKind::FlagPredicate(FlagAtom::Perm))
    } else if token.matches("-flags") {
        Some(AtomKind::FlagPredicate(FlagAtom::Flags))
    } else if token.matches("-reparse-type") {
        Some(AtomKind::FlagPredicate(FlagAtom::ReparseType))
    } else if token.matches("-size") {
        Some(AtomKind::Size)
    } else if token.matches("-empty") {
        Some(AtomKind::Empty)
    } else if token.matches("-inum") {
        Some(AtomKind::Identity(IdentityAtom::Inum))
    } else if token.matches("-links") {
        Some(AtomKind::Identity(IdentityAtom::Links))
    } else if token.matches("-samefile") {
        Some(AtomKind::Identity(IdentityAtom::SameFile))
    } else {
        None
    }
}

fn classify_time_atom(token: Arg<'_>) -> Option<AtomKind> {
    if token.matches("-used") {
        Some(AtomKind::Used)
    } else if token.matches("-atime") {
        Some(AtomKind::Time(TimeAtom::ATime))
    } else if token.matches("-ctime") {
        Some(AtomKind::Time(TimeAtom::CTime))
    } else if token.matches("-mtime") {
        Some(AtomKind::Time(TimeAtom::MTime))
    } else if token.matches("-amin") {
        Some(AtomKind::Time(TimeAtom::AMin))
    } else if token.matches("-cmin") {
        Some(AtomKind::Time(TimeAtom::CMin))
    } else if token.matches("-mmin") {
        Some(AtomKind::Time(TimeAtom::MMin))
    } else if token.matches("-newer") {
        Some(AtomKind::Newer(NewerAtom::Newer))
    } else if token.matches("-anewer") {
        Some(AtomKind::Newer(NewerAtom::ANewer))
    } else if token.matches("-cnewer") {
        Some(AtomKind::Newer(NewerAtom::CNewer))
    } else {
        parse_newerxy_flag(token)
            .map(|(current, reference)| AtomKind::Newer(NewerAtom::NewerXY { current, reference }))
    }
}

fn classify_action_atom(token: Arg<'_>) -> Option<AtomKind> {
    if token.matches("-print") {
        Some(AtomKind::Output(OutputAtom::Print))
    } else if token.matches("-print0") {
        Some(AtomKind::Output(OutputAtom::Print0))
    } else if token.matches("-printf") {
        Some(AtomKind::Output(OutputAtom::Printf))
    } else if token.matches("-ls") {
        Some(AtomKind::Output(OutputAtom::Ls))
    } else if token.matches("-fprint") {
        Some(AtomKind::FileOutput(FileOutputAtom::FPrint))
    } else if token.matches("-fprint0") {
        Some(AtomKind::FileOutput(FileOutputAtom::FPrint0))
    } else if token.matches("-fprintf") {
        Some(AtomKind::FileOutput(FileOutputAtom::FPrintf))
    } else if token.matches("-fls") {
        Some(AtomKind::FileOutput(FileOutputAtom::Fls))
    } else if token.matches("-quit") {
        Some(AtomKind::Quit)
    } else if token.matches("-exec") {
        Some(AtomKind::Exec(ExecAtom::Exec))
    } else if token.matches("-execdir") {
        Some(AtomKind::Exec(ExecAtom::ExecDir))
    } else if token.matches("-ok") {
        Some(AtomKind::Exec(ExecAtom::Ok))
    } else if token.matches("-okdir") {
        Some(AtomKind::Exec(ExecAtom::OkDir))
    } else if token.matches("-delete") {
        Some(AtomKind::Delete)
    } else {
        None
    }
}

fn classify_compatibility_atom(token: Arg<'_>) -> Option<AtomKind> {
    if token.matches("-files0-from") {
        Some(AtomKind::Compatibility(CompatibilityAtom::Files0From))
    } else if token.matches("-follow") {
        Some(AtomKind::Compatibility(CompatibilityAtom::Follow))
    } else if token.matches("-noleaf") {
        Some(AtomKind::Compatibility(CompatibilityAtom::NoLeaf))
    } else if token.matches("-warn") {
        Some(AtomKind::Compatibility(CompatibilityAtom::Warn))
    } else if token.matches("-nowarn") {
        Some(AtomKind::Compatibility(CompatibilityAtom::NoWarn))
    } else if token.matches("-ignore_readdir_race") {
        Some(AtomKind::Compatibility(
            CompatibilityAtom::IgnoreReaddirRace,
        ))
    } else if token.matches("-noignore_readdir_race") {
        Some(AtomKind::Compatibility(
            CompatibilityAtom::NoIgnoreReaddirRace,
        ))
    } else {
        None
    }
}

struct Parser<'a> {
    args: ArgCursor<'a>,
    compatibility_options: CompatibilityOptions,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [OsString], compatibility_options: CompatibilityOptions) -> Self {
        Self {
            args: ArgCursor::new(tokens),
            compatibility_options,
        }
    }

    fn into_compatibility_options(self) -> CompatibilityOptions {
        self.compatibility_options
    }

    fn peek(&self) -> Option<Arg<'a>> {
        self.args.peek()
    }

    fn bump(&mut self) -> Option<Arg<'a>> {
        self.args.bump()
    }

    fn expect_end(&self) -> Result<(), Diagnostic> {
        if let Some(token) = self.peek() {
            return Err(Diagnostic::parse(format!(
                "unexpected trailing token `{}`",
                token.display()
            )));
        }

        Ok(())
    }

    fn parse_sequence_expression(&mut self) -> Result<Expr, Diagnostic> {
        let mut items = vec![self.parse_or_expression()?];

        while self.peek().is_some_and(|token| token.matches(",")) {
            self.bump();
            items.push(self.parse_or_expression()?);
        }

        if items.len() == 1 {
            Ok(items.remove(0))
        } else {
            Ok(Expr::Sequence(items))
        }
    }

    fn parse_or_expression(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.parse_and_expression()?;

        while self
            .peek()
            .is_some_and(|token| token.matches("-o") || token.matches("-or"))
        {
            self.bump();
            let right = self.parse_and_expression()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_and_expression(&mut self) -> Result<Expr, Diagnostic> {
        let mut items = vec![self.parse_not_expression()?];

        loop {
            if self
                .peek()
                .is_some_and(|token| token.matches("-a") || token.matches("-and"))
            {
                self.bump();
                items.push(self.parse_not_expression()?);
                continue;
            }

            if self.starts_primary() {
                items.push(self.parse_not_expression()?);
                continue;
            }

            break;
        }

        if items.len() == 1 {
            Ok(items.remove(0))
        } else {
            Ok(Expr::And(items))
        }
    }

    fn parse_not_expression(&mut self) -> Result<Expr, Diagnostic> {
        if self
            .peek()
            .is_some_and(|token| token.matches("!") || token.matches("-not"))
        {
            self.bump();
            return Ok(Expr::Not(Box::new(self.parse_not_expression()?)));
        }

        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        match self.peek() {
            Some(token) if token.matches("(") => {
                self.bump();
                let expr = self.parse_sequence_expression()?;
                match self.bump() {
                    Some(token) if token.matches(")") => Ok(expr),
                    _ => Err(Diagnostic::parse("expected `)` to close group")),
                }
            }
            Some(token) if token.matches(")") => Err(Diagnostic::parse("unexpected `)`")),
            _ => self.parse_atom(),
        }
    }

    fn parse_atom(&mut self) -> Result<Expr, Diagnostic> {
        let token = self
            .bump()
            .ok_or_else(|| Diagnostic::parse("expected predicate or action"))?;
        let token_display = token.display();
        let kind = classify_atom(token).ok_or_else(|| {
            Diagnostic::parse(format!("unsupported expression token `{}`", token_display))
        })?;
        self.parse_classified_atom(kind)
    }

    fn parse_classified_atom(&mut self, kind: AtomKind) -> Result<Expr, Diagnostic> {
        Ok(match kind {
            AtomKind::MaxDepth => Expr::Predicate(Predicate::MaxDepth(self.take_u32("-maxdepth")?)),
            AtomKind::MinDepth => Expr::Predicate(Predicate::MinDepth(self.take_u32("-mindepth")?)),
            AtomKind::Depth => Expr::Predicate(Predicate::Depth),
            AtomKind::Prune => Expr::Predicate(Predicate::Prune),
            AtomKind::XDev => Expr::Predicate(Predicate::XDev),
            AtomKind::Access(atom) => self.parse_access_atom(atom),
            AtomKind::Glob(atom) => self.parse_glob_atom(atom)?,
            AtomKind::Regex {
                case_insensitive,
                flag,
            } => Expr::Predicate(Predicate::Regex {
                pattern: self.take_os_string(flag)?,
                case_insensitive,
            }),
            AtomKind::RegexType => {
                Expr::Predicate(Predicate::RegexType(self.take_os_string("-regextype")?))
            }
            AtomKind::FsType => Expr::Predicate(Predicate::FsType(self.take_os_string("-fstype")?)),
            AtomKind::Context => {
                Expr::Predicate(Predicate::Context(self.take_os_string("-context")?))
            }
            AtomKind::LinkGlob {
                case_insensitive,
                flag,
            } => Expr::Predicate(Predicate::LName {
                pattern: self.take_os_string(flag)?,
                case_insensitive,
            }),
            AtomKind::Ownership(atom) => self.parse_ownership_atom(atom)?,
            AtomKind::FlagPredicate(atom) => self.parse_flag_atom(atom)?,
            AtomKind::Size => Expr::Predicate(Predicate::Size(self.take_os_string("-size")?)),
            AtomKind::Empty => Expr::Predicate(Predicate::Empty),
            AtomKind::Used => Expr::Predicate(self.parse_validated_time("-used", Predicate::Used)?),
            AtomKind::Identity(atom) => self.parse_identity_atom(atom)?,
            AtomKind::Time(atom) => self.parse_time_atom(atom)?,
            AtomKind::Newer(atom) => self.parse_newer_atom(atom)?,
            AtomKind::DayStart => Expr::Predicate(Predicate::DayStart),
            AtomKind::Type { follow_symlinks } => self.parse_type_atom(follow_symlinks)?,
            AtomKind::Boolean(value) => Expr::Predicate(if value {
                Predicate::True
            } else {
                Predicate::False
            }),
            AtomKind::Output(atom) => self.parse_output_atom(atom)?,
            AtomKind::FileOutput(atom) => self.parse_file_output_atom(atom)?,
            AtomKind::Compatibility(atom) => self.parse_compatibility_atom(atom)?,
            AtomKind::Quit => Expr::Action(Action::Quit),
            AtomKind::Exec(atom) => Expr::Action(self.parse_exec_atom(atom)?),
            AtomKind::Delete => Expr::Action(Action::Delete),
        })
    }

    fn parse_access_atom(&mut self, atom: AccessAtom) -> Expr {
        Expr::Predicate(match atom {
            AccessAtom::Readable => Predicate::Readable,
            AccessAtom::Writable => Predicate::Writable,
            AccessAtom::Executable => Predicate::Executable,
        })
    }

    fn parse_glob_atom(&mut self, atom: GlobAtom) -> Result<Expr, Diagnostic> {
        Ok(Expr::Predicate(match atom {
            GlobAtom::Name {
                case_insensitive,
                flag,
            } => Predicate::Name {
                pattern: self.take_os_string(flag)?,
                case_insensitive,
            },
            GlobAtom::Path {
                case_insensitive,
                flag,
            } => Predicate::Path {
                pattern: self.take_os_string(flag)?,
                case_insensitive,
            },
        }))
    }

    fn parse_ownership_atom(&mut self, atom: OwnershipAtom) -> Result<Expr, Diagnostic> {
        Ok(Expr::Predicate(match atom {
            OwnershipAtom::Uid => self.parse_validated_numeric("-uid", Predicate::Uid)?,
            OwnershipAtom::Gid => self.parse_validated_numeric("-gid", Predicate::Gid)?,
            OwnershipAtom::User => Predicate::User(self.take_os_string("-user")?),
            OwnershipAtom::Group => Predicate::Group(self.take_os_string("-group")?),
            OwnershipAtom::Owner => Predicate::Owner(self.take_os_string("-owner")?),
            OwnershipAtom::OwnerSid => Predicate::OwnerSid(self.take_os_string("-owner-sid")?),
            OwnershipAtom::GroupSid => Predicate::GroupSid(self.take_os_string("-group-sid")?),
            OwnershipAtom::NoUser => Predicate::NoUser,
            OwnershipAtom::NoGroup => Predicate::NoGroup,
        }))
    }

    fn parse_flag_atom(&mut self, atom: FlagAtom) -> Result<Expr, Diagnostic> {
        Ok(Expr::Predicate(match atom {
            FlagAtom::Perm => Predicate::Perm(self.take_os_string("-perm")?),
            FlagAtom::Flags => Predicate::Flags(self.take_os_string("-flags")?),
            FlagAtom::ReparseType => Predicate::ReparseType(self.take_os_string("-reparse-type")?),
        }))
    }

    fn parse_identity_atom(&mut self, atom: IdentityAtom) -> Result<Expr, Diagnostic> {
        Ok(Expr::Predicate(match atom {
            IdentityAtom::Inum => self.parse_validated_numeric("-inum", Predicate::Inum)?,
            IdentityAtom::Links => self.parse_validated_numeric("-links", Predicate::Links)?,
            IdentityAtom::SameFile => {
                Predicate::SameFile(PathBuf::from(self.take_os_string("-samefile")?))
            }
        }))
    }

    fn parse_time_atom(&mut self, atom: TimeAtom) -> Result<Expr, Diagnostic> {
        let predicate = match atom {
            TimeAtom::ATime => self.parse_validated_time("-atime", Predicate::ATime)?,
            TimeAtom::CTime => self.parse_validated_time("-ctime", Predicate::CTime)?,
            TimeAtom::MTime => self.parse_validated_time("-mtime", Predicate::MTime)?,
            TimeAtom::AMin => self.parse_validated_time("-amin", Predicate::AMin)?,
            TimeAtom::CMin => self.parse_validated_time("-cmin", Predicate::CMin)?,
            TimeAtom::MMin => self.parse_validated_time("-mmin", Predicate::MMin)?,
        };
        Ok(Expr::Predicate(predicate))
    }

    fn parse_newer_atom(&mut self, atom: NewerAtom) -> Result<Expr, Diagnostic> {
        Ok(Expr::Predicate(match atom {
            NewerAtom::Newer => Predicate::Newer(PathBuf::from(self.take_os_string("-newer")?)),
            NewerAtom::ANewer => Predicate::ANewer(PathBuf::from(self.take_os_string("-anewer")?)),
            NewerAtom::CNewer => Predicate::CNewer(PathBuf::from(self.take_os_string("-cnewer")?)),
            NewerAtom::NewerXY { current, reference } => {
                let flag = format!("-newer{current}{reference}");
                Predicate::NewerXY {
                    current,
                    reference,
                    reference_arg: self.take_os_string(flag.as_str())?,
                }
            }
        }))
    }

    fn parse_type_atom(&mut self, follow_symlinks: bool) -> Result<Expr, Diagnostic> {
        let flag = if follow_symlinks { "-xtype" } else { "-type" };
        let filter = self.take_type_matcher_for(flag)?;
        Ok(Expr::Predicate(if follow_symlinks {
            Predicate::XType(filter)
        } else {
            Predicate::Type(filter)
        }))
    }

    fn parse_compatibility_atom(&mut self, atom: CompatibilityAtom) -> Result<Expr, Diagnostic> {
        let predicate = match atom {
            CompatibilityAtom::Files0From => {
                let value = self.take_os_string("-files0-from")?;
                self.compatibility_options.files0_from =
                    Some(if value.as_os_str() == OsStr::new("-") {
                        Files0From::Stdin
                    } else {
                        Files0From::Path(PathBuf::from(value))
                    });
                CompatibilityPredicate::Files0From
            }
            CompatibilityAtom::Follow => {
                self.compatibility_options.follow = true;
                CompatibilityPredicate::Follow
            }
            CompatibilityAtom::NoLeaf => {
                self.compatibility_options.noleaf = true;
                CompatibilityPredicate::NoLeaf
            }
            CompatibilityAtom::Warn => {
                self.compatibility_options.warning_mode = WarningMode::Warn;
                CompatibilityPredicate::Warn
            }
            CompatibilityAtom::NoWarn => {
                self.compatibility_options.warning_mode = WarningMode::NoWarn;
                CompatibilityPredicate::NoWarn
            }
            CompatibilityAtom::IgnoreReaddirRace => {
                self.compatibility_options.ignore_readdir_race = Some(true);
                CompatibilityPredicate::IgnoreReaddirRace
            }
            CompatibilityAtom::NoIgnoreReaddirRace => {
                self.compatibility_options.ignore_readdir_race = Some(false);
                CompatibilityPredicate::NoIgnoreReaddirRace
            }
        };

        Ok(Expr::Predicate(Predicate::Compatibility(predicate)))
    }

    fn parse_output_atom(&mut self, atom: OutputAtom) -> Result<Expr, Diagnostic> {
        Ok(Expr::Action(match atom {
            OutputAtom::Print => Action::Print,
            OutputAtom::Print0 => Action::Print0,
            OutputAtom::Printf => Action::Printf {
                format: self.take_os_string("-printf")?,
            },
            OutputAtom::Ls => Action::Ls,
        }))
    }

    fn parse_file_output_atom(&mut self, atom: FileOutputAtom) -> Result<Expr, Diagnostic> {
        Ok(Expr::Action(match atom {
            FileOutputAtom::FPrint => Action::FPrint {
                path: PathBuf::from(self.take_os_string("-fprint")?),
            },
            FileOutputAtom::FPrint0 => Action::FPrint0 {
                path: PathBuf::from(self.take_os_string("-fprint0")?),
            },
            FileOutputAtom::FPrintf => Action::FPrintf {
                path: PathBuf::from(self.take_os_string("-fprintf")?),
                format: self.take_os_string("-fprintf")?,
            },
            FileOutputAtom::Fls => Action::Fls {
                path: PathBuf::from(self.take_os_string("-fls")?),
            },
        }))
    }

    fn parse_exec_atom(&mut self, atom: ExecAtom) -> Result<Action, Diagnostic> {
        match atom {
            ExecAtom::Exec => self.take_exec_action(false, false),
            ExecAtom::ExecDir => self.take_exec_action(true, false),
            ExecAtom::Ok => self.take_exec_action(false, true),
            ExecAtom::OkDir => self.take_exec_action(true, true),
        }
    }

    fn parse_validated_numeric(
        &mut self,
        flag: &str,
        build: fn(OsString) -> Predicate,
    ) -> Result<Predicate, Diagnostic> {
        let raw = self.take_os_string(flag)?;
        validate_numeric_argument(flag, raw.as_os_str())?;
        Ok(build(raw))
    }

    fn parse_validated_time(
        &mut self,
        flag: &str,
        build: fn(OsString) -> Predicate,
    ) -> Result<Predicate, Diagnostic> {
        let raw = self.take_os_string(flag)?;
        validate_time_argument(flag, raw.as_os_str())?;
        Ok(build(raw))
    }

    fn take_os_string(&mut self, flag: &str) -> Result<OsString, Diagnostic> {
        self.bump()
            .map(Arg::to_os_string)
            .ok_or_else(|| Diagnostic::parse(format!("missing argument for `{flag}`")))
    }

    fn take_u32(&mut self, flag: &str) -> Result<u32, Diagnostic> {
        let value = self.take_os_string(flag)?;
        let rendered = value.to_string_lossy();
        rendered.parse::<u32>().map_err(|_| {
            Diagnostic::parse(format!(
                "invalid numeric argument for `{flag}`: `{rendered}`"
            ))
        })
    }

    fn take_type_matcher_for(&mut self, flag: &str) -> Result<FileTypeMatcher, Diagnostic> {
        let value = self.take_os_string(flag)?;
        let bytes = value.as_os_str().as_encoded_bytes();
        let mut filters = Vec::new();

        for component in bytes.split(|byte| *byte == b',') {
            filters.push(parse_type_filter_component(flag, component, &value)?);
        }

        Ok(FileTypeMatcher::from_filters(filters))
    }

    fn take_exec_action(
        &mut self,
        chdir_before_exec: bool,
        prompt: bool,
    ) -> Result<Action, Diagnostic> {
        let mut argv = Vec::new();
        let mut batch = false;

        loop {
            let token = self
                .bump()
                .ok_or_else(|| Diagnostic::parse("unterminated exec-style action"))?;

            if token.matches(";") {
                break;
            }

            if token.matches("+") {
                batch = true;
                break;
            }

            argv.push(token.to_os_string());
        }

        Ok(match (chdir_before_exec, prompt) {
            (false, false) => Action::Exec { argv, batch },
            (true, false) => Action::ExecDir { argv, batch },
            (false, true) => Action::Ok { argv, batch },
            (true, true) => Action::OkDir { argv, batch },
        })
    }

    fn starts_primary(&self) -> bool {
        matches!(
            self.peek(),
            Some(token)
                if !token.matches(")")
                    && !token.matches(",")
                    && !token.matches("-o")
                    && !token.matches("-or")
        )
    }
}

fn parse_newerxy_flag(token: Arg<'_>) -> Option<(char, char)> {
    let token = token.to_os_string();
    let bytes = token.as_encoded_bytes();
    if bytes.len() != 8 || &bytes[..6] != b"-newer" {
        return None;
    }

    Some((char::from(bytes[6]), char::from(bytes[7])))
}

fn parse_type_filter_component(
    flag: &str,
    component: &[u8],
    full_value: &OsString,
) -> Result<FileTypeFilter, Diagnostic> {
    match component {
        b"f" => Ok(FileTypeFilter::File),
        b"d" => Ok(FileTypeFilter::Directory),
        b"l" => Ok(FileTypeFilter::Symlink),
        b"b" => Ok(FileTypeFilter::Block),
        b"c" => Ok(FileTypeFilter::Character),
        b"p" => Ok(FileTypeFilter::Fifo),
        b"s" => Ok(FileTypeFilter::Socket),
        b"D" => Ok(FileTypeFilter::Door),
        b"" => Err(Diagnostic::parse(format!(
            "empty file type in {flag} list `{}`",
            full_value.to_string_lossy()
        ))),
        other => Err(Diagnostic::parse(format!(
            "unsupported {flag} value `{}`",
            String::from_utf8_lossy(other)
        ))),
    }
}
