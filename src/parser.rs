use crate::args::{Arg, ArgCursor};
use crate::ast::{Action, CommandAst, Expr, FileTypeFilter, GlobalOption, Predicate};
use crate::diagnostics::Diagnostic;
use crate::follow::FollowMode;
use crate::numeric::validate_numeric_argument;
use crate::time::validate_time_argument;
use std::ffi::OsString;
use std::path::PathBuf;

pub fn parse_command(argv: &[OsString]) -> Result<CommandAst, Diagnostic> {
    let mut global_options = Vec::new();
    let mut index = 0;

    while let Some(option) = argv
        .get(index)
        .map(|arg| parse_global_option(Arg::new(arg.as_os_str())))
        .flatten()
    {
        global_options.push(option);
        index += 1;
    }

    let path_count = argv[index..]
        .iter()
        .position(|arg| is_expression_start(Arg::new(arg.as_os_str())))
        .unwrap_or(argv[index..].len());
    let split_index = index + path_count;

    let start_paths = if path_count == 0 {
        vec![PathBuf::from(".")]
    } else {
        argv[index..split_index].iter().map(PathBuf::from).collect()
    };

    let expr = if split_index == argv.len() {
        Expr::Action(Action::Print)
    } else {
        let mut parser = Parser::new(&argv[split_index..]);
        let expr = parser.parse_or_expression()?;
        parser.expect_end()?;
        expr
    };

    Ok(CommandAst {
        start_paths,
        global_options,
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
    } else {
        None
    }
}

struct Parser<'a> {
    args: ArgCursor<'a>,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [OsString]) -> Self {
        Self {
            args: ArgCursor::new(tokens),
        }
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
                let expr = self.parse_or_expression()?;
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

        let expr = if token.matches("-maxdepth") {
            Expr::Predicate(Predicate::MaxDepth(self.take_u32("-maxdepth")?))
        } else if token.matches("-mindepth") {
            Expr::Predicate(Predicate::MinDepth(self.take_u32("-mindepth")?))
        } else if token.matches("-depth") {
            Expr::Predicate(Predicate::Depth)
        } else if token.matches("-prune") {
            Expr::Predicate(Predicate::Prune)
        } else if token.matches("-xdev") || token.matches("-mount") {
            Expr::Predicate(Predicate::XDev)
        } else if token.matches("-readable") {
            Expr::Predicate(Predicate::Readable)
        } else if token.matches("-writable") {
            Expr::Predicate(Predicate::Writable)
        } else if token.matches("-executable") {
            Expr::Predicate(Predicate::Executable)
        } else if token.matches("-name") {
            Expr::Predicate(Predicate::Name {
                pattern: self.take_os_string("-name")?,
                case_insensitive: false,
            })
        } else if token.matches("-iname") {
            Expr::Predicate(Predicate::Name {
                pattern: self.take_os_string("-iname")?,
                case_insensitive: true,
            })
        } else if token.matches("-path") || token.matches("-wholename") {
            Expr::Predicate(Predicate::Path {
                pattern: self.take_os_string(token_display.as_str())?,
                case_insensitive: false,
            })
        } else if token.matches("-ipath") || token.matches("-iwholename") {
            Expr::Predicate(Predicate::Path {
                pattern: self.take_os_string(token_display.as_str())?,
                case_insensitive: true,
            })
        } else if token.matches("-regex") {
            Expr::Predicate(Predicate::Regex {
                pattern: self.take_os_string("-regex")?,
                case_insensitive: false,
            })
        } else if token.matches("-iregex") {
            Expr::Predicate(Predicate::Regex {
                pattern: self.take_os_string("-iregex")?,
                case_insensitive: true,
            })
        } else if token.matches("-regextype") {
            Expr::Predicate(Predicate::RegexType(self.take_os_string("-regextype")?))
        } else if token.matches("-fstype") {
            Expr::Predicate(Predicate::FsType(self.take_os_string("-fstype")?))
        } else if token.matches("-lname") {
            Expr::Predicate(Predicate::LName {
                pattern: self.take_os_string("-lname")?,
                case_insensitive: false,
            })
        } else if token.matches("-ilname") {
            Expr::Predicate(Predicate::LName {
                pattern: self.take_os_string("-ilname")?,
                case_insensitive: true,
            })
        } else if token.matches("-uid") {
            let raw = self.take_os_string("-uid")?;
            validate_numeric_argument("-uid", raw.as_os_str())?;
            Expr::Predicate(Predicate::Uid(raw))
        } else if token.matches("-gid") {
            let raw = self.take_os_string("-gid")?;
            validate_numeric_argument("-gid", raw.as_os_str())?;
            Expr::Predicate(Predicate::Gid(raw))
        } else if token.matches("-user") {
            Expr::Predicate(Predicate::User(self.take_os_string("-user")?))
        } else if token.matches("-group") {
            Expr::Predicate(Predicate::Group(self.take_os_string("-group")?))
        } else if token.matches("-nouser") {
            Expr::Predicate(Predicate::NoUser)
        } else if token.matches("-nogroup") {
            Expr::Predicate(Predicate::NoGroup)
        } else if token.matches("-perm") {
            Expr::Predicate(Predicate::Perm(self.take_os_string("-perm")?))
        } else if token.matches("-size") {
            Expr::Predicate(Predicate::Size(self.take_os_string("-size")?))
        } else if token.matches("-empty") {
            Expr::Predicate(Predicate::Empty)
        } else if token.matches("-used") {
            let raw = self.take_os_string("-used")?;
            validate_time_argument("-used", raw.as_os_str())?;
            Expr::Predicate(Predicate::Used(raw))
        } else if token.matches("-inum") {
            let raw = self.take_os_string("-inum")?;
            validate_numeric_argument("-inum", raw.as_os_str())?;
            Expr::Predicate(Predicate::Inum(raw))
        } else if token.matches("-links") {
            let raw = self.take_os_string("-links")?;
            validate_numeric_argument("-links", raw.as_os_str())?;
            Expr::Predicate(Predicate::Links(raw))
        } else if token.matches("-samefile") {
            Expr::Predicate(Predicate::SameFile(PathBuf::from(
                self.take_os_string("-samefile")?,
            )))
        } else if token.matches("-atime") {
            let raw = self.take_os_string("-atime")?;
            validate_time_argument("-atime", raw.as_os_str())?;
            Expr::Predicate(Predicate::ATime(raw))
        } else if token.matches("-ctime") {
            let raw = self.take_os_string("-ctime")?;
            validate_time_argument("-ctime", raw.as_os_str())?;
            Expr::Predicate(Predicate::CTime(raw))
        } else if token.matches("-mtime") {
            let raw = self.take_os_string("-mtime")?;
            validate_time_argument("-mtime", raw.as_os_str())?;
            Expr::Predicate(Predicate::MTime(raw))
        } else if token.matches("-amin") {
            let raw = self.take_os_string("-amin")?;
            validate_time_argument("-amin", raw.as_os_str())?;
            Expr::Predicate(Predicate::AMin(raw))
        } else if token.matches("-cmin") {
            let raw = self.take_os_string("-cmin")?;
            validate_time_argument("-cmin", raw.as_os_str())?;
            Expr::Predicate(Predicate::CMin(raw))
        } else if token.matches("-mmin") {
            let raw = self.take_os_string("-mmin")?;
            validate_time_argument("-mmin", raw.as_os_str())?;
            Expr::Predicate(Predicate::MMin(raw))
        } else if token.matches("-newer") {
            Expr::Predicate(Predicate::Newer(PathBuf::from(
                self.take_os_string("-newer")?,
            )))
        } else if token.matches("-anewer") {
            Expr::Predicate(Predicate::ANewer(PathBuf::from(
                self.take_os_string("-anewer")?,
            )))
        } else if token.matches("-cnewer") {
            Expr::Predicate(Predicate::CNewer(PathBuf::from(
                self.take_os_string("-cnewer")?,
            )))
        } else if let Some((current, reference)) = parse_newerxy_flag(token) {
            Expr::Predicate(Predicate::NewerXY {
                current,
                reference,
                reference_arg: self.take_os_string(token_display.as_str())?,
            })
        } else if token.matches("-daystart") {
            Expr::Predicate(Predicate::DayStart)
        } else if token.matches("-type") {
            Expr::Predicate(Predicate::Type(self.take_type_filter()?))
        } else if token.matches("-xtype") {
            Expr::Predicate(Predicate::XType(self.take_type_filter_for("-xtype")?))
        } else if token.matches("-true") {
            Expr::Predicate(Predicate::True)
        } else if token.matches("-false") {
            Expr::Predicate(Predicate::False)
        } else if token.matches("-print") {
            Expr::Action(Action::Print)
        } else if token.matches("-print0") {
            Expr::Action(Action::Print0)
        } else if token.matches("-printf") {
            Expr::Action(Action::Printf {
                format: self.take_os_string("-printf")?,
            })
        } else if token.matches("-exec") {
            Expr::Action(self.take_exec_action(false, false)?)
        } else if token.matches("-execdir") {
            Expr::Action(self.take_exec_action(true, false)?)
        } else if token.matches("-ok") {
            Expr::Action(self.take_exec_action(false, true)?)
        } else if token.matches("-okdir") {
            Expr::Action(self.take_exec_action(true, true)?)
        } else if token.matches("-delete") {
            Expr::Action(Action::Delete)
        } else {
            return Err(Diagnostic::parse(format!(
                "unsupported token in parser subset `{}`",
                token_display
            )));
        };

        Ok(expr)
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

    fn take_type_filter(&mut self) -> Result<FileTypeFilter, Diagnostic> {
        self.take_type_filter_for("-type")
    }

    fn take_type_filter_for(&mut self, flag: &str) -> Result<FileTypeFilter, Diagnostic> {
        let value = self.take_os_string(flag)?;
        match value.as_os_str().as_encoded_bytes() {
            b"f" => Ok(FileTypeFilter::File),
            b"d" => Ok(FileTypeFilter::Directory),
            b"l" => Ok(FileTypeFilter::Symlink),
            b"b" => Ok(FileTypeFilter::Block),
            b"c" => Ok(FileTypeFilter::Character),
            b"p" => Ok(FileTypeFilter::Fifo),
            b"s" => Ok(FileTypeFilter::Socket),
            _ => Err(Diagnostic::parse(format!(
                "unsupported {flag} value `{}`",
                value.to_string_lossy()
            ))),
        }
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
            (false, true) => Action::Ok { argv },
            (true, true) => Action::OkDir { argv },
        })
    }

    fn starts_primary(&self) -> bool {
        matches!(
            self.peek(),
            Some(token) if !token.matches(")") && !token.matches("-o") && !token.matches("-or")
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
