use crate::args::{Arg, ArgCursor};
use crate::ast::{Action, CommandAst, Expr, FileTypeFilter, Predicate};
use crate::diagnostics::Diagnostic;
use std::ffi::OsString;
use std::path::PathBuf;

pub fn parse_command(argv: &[OsString]) -> Result<CommandAst, Diagnostic> {
    let split_index = argv
        .iter()
        .position(|arg| is_expression_start(Arg::new(arg.as_os_str())))
        .unwrap_or(argv.len());

    let start_paths = if split_index == 0 {
        vec![PathBuf::from(".")]
    } else {
        argv[..split_index].iter().map(PathBuf::from).collect()
    };

    let expr = if split_index == argv.len() {
        Expr::Action(Action::Print)
    } else {
        let mut parser = Parser::new(&argv[split_index..]);
        let expr = parser.parse_or_expression()?;
        parser.expect_end()?;
        expr
    };

    Ok(CommandAst { start_paths, expr })
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

        let expr = if token.matches("-maxdepth") {
            Expr::Predicate(Predicate::MaxDepth(self.take_u32("-maxdepth")?))
        } else if token.matches("-mindepth") {
            Expr::Predicate(Predicate::MinDepth(self.take_u32("-mindepth")?))
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
        } else if token.matches("-path") {
            Expr::Predicate(Predicate::Path {
                pattern: self.take_os_string("-path")?,
                case_insensitive: false,
            })
        } else if token.matches("-ipath") {
            Expr::Predicate(Predicate::Path {
                pattern: self.take_os_string("-ipath")?,
                case_insensitive: true,
            })
        } else if token.matches("-type") {
            Expr::Predicate(Predicate::Type(self.take_type_filter()?))
        } else if token.matches("-true") {
            Expr::Predicate(Predicate::True)
        } else if token.matches("-false") {
            Expr::Predicate(Predicate::False)
        } else if token.matches("-print") {
            Expr::Action(Action::Print)
        } else if token.matches("-print0") {
            Expr::Action(Action::Print0)
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
                token.display()
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
        let value = self.take_os_string("-type")?;
        match value.as_os_str().as_encoded_bytes() {
            b"f" => Ok(FileTypeFilter::File),
            b"d" => Ok(FileTypeFilter::Directory),
            b"l" => Ok(FileTypeFilter::Symlink),
            b"b" => Ok(FileTypeFilter::Block),
            b"c" => Ok(FileTypeFilter::Character),
            b"p" => Ok(FileTypeFilter::Fifo),
            b"s" => Ok(FileTypeFilter::Socket),
            _ => Err(Diagnostic::parse(format!(
                "unsupported -type value `{}`",
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
