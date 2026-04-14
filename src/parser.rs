use crate::ast::{Action, CommandAst, Expr, FileTypeFilter, Predicate};
use crate::diagnostics::Diagnostic;
use std::ffi::OsString;
use std::path::PathBuf;

pub fn parse_command(argv: &[OsString]) -> Result<CommandAst, Diagnostic> {
    let split_index = argv
        .iter()
        .position(is_expression_start)
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

fn is_expression_start(arg: &OsString) -> bool {
    matches!(
        arg.to_string_lossy().as_ref(),
        "!" | "(" | ")" | "-a" | "-and" | "-o" | "-or" | "-not"
    ) || arg.to_string_lossy().starts_with('-')
}

struct Parser<'a> {
    tokens: &'a [OsString],
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [OsString]) -> Self {
        Self { tokens, index: 0 }
    }

    fn peek(&self) -> Option<String> {
        self.tokens
            .get(self.index)
            .map(|value| value.to_string_lossy().into_owned())
    }

    fn bump(&mut self) -> Option<String> {
        let value = self.peek();
        if value.is_some() {
            self.index += 1;
        }
        value
    }

    fn expect_end(&self) -> Result<(), Diagnostic> {
        if let Some(token) = self.peek() {
            return Err(Diagnostic::new(
                format!("unexpected trailing token `{token}`"),
                1,
            ));
        }

        Ok(())
    }

    fn parse_or_expression(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.parse_and_expression()?;

        while matches!(self.peek().as_deref(), Some("-o" | "-or")) {
            self.bump();
            let right = self.parse_and_expression()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_and_expression(&mut self) -> Result<Expr, Diagnostic> {
        let mut items = vec![self.parse_not_expression()?];

        loop {
            if matches!(self.peek().as_deref(), Some("-a" | "-and")) {
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
        if matches!(self.peek().as_deref(), Some("!" | "-not")) {
            self.bump();
            return Ok(Expr::Not(Box::new(self.parse_not_expression()?)));
        }

        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        match self.peek().as_deref() {
            Some("(") => {
                self.bump();
                let expr = self.parse_or_expression()?;
                match self.bump().as_deref() {
                    Some(")") => Ok(expr),
                    _ => Err(Diagnostic::new("expected `)` to close group", 1)),
                }
            }
            Some(")") => Err(Diagnostic::new("unexpected `)`", 1)),
            _ => self.parse_atom(),
        }
    }

    fn parse_atom(&mut self) -> Result<Expr, Diagnostic> {
        let token = self
            .bump()
            .ok_or_else(|| Diagnostic::new("expected predicate or action", 1))?;

        let expr = match token.as_str() {
            "-maxdepth" => Expr::Predicate(Predicate::MaxDepth(self.take_u32("-maxdepth")?)),
            "-mindepth" => Expr::Predicate(Predicate::MinDepth(self.take_u32("-mindepth")?)),
            "-name" => Expr::Predicate(Predicate::Name {
                pattern: self.take_string("-name")?,
                case_insensitive: false,
            }),
            "-iname" => Expr::Predicate(Predicate::Name {
                pattern: self.take_string("-iname")?,
                case_insensitive: true,
            }),
            "-path" => Expr::Predicate(Predicate::Path {
                pattern: self.take_string("-path")?,
                case_insensitive: false,
            }),
            "-ipath" => Expr::Predicate(Predicate::Path {
                pattern: self.take_string("-ipath")?,
                case_insensitive: true,
            }),
            "-type" => Expr::Predicate(Predicate::Type(self.take_type_filter()?)),
            "-true" => Expr::Predicate(Predicate::True),
            "-false" => Expr::Predicate(Predicate::False),
            "-print" => Expr::Action(Action::Print),
            "-print0" => Expr::Action(Action::Print0),
            "-exec" => Expr::Action(self.take_exec_action(false, false)?),
            "-execdir" => Expr::Action(self.take_exec_action(true, false)?),
            "-ok" => Expr::Action(self.take_exec_action(false, true)?),
            "-okdir" => Expr::Action(self.take_exec_action(true, true)?),
            "-delete" => Expr::Action(Action::Delete),
            other => {
                return Err(Diagnostic::new(
                    format!("unsupported token in parser subset `{other}`"),
                    1,
                ));
            }
        };

        Ok(expr)
    }

    fn take_string(&mut self, flag: &str) -> Result<String, Diagnostic> {
        self.bump()
            .ok_or_else(|| Diagnostic::new(format!("missing argument for `{flag}`"), 1))
    }

    fn take_u32(&mut self, flag: &str) -> Result<u32, Diagnostic> {
        let value = self.take_string(flag)?;
        value.parse::<u32>().map_err(|_| {
            Diagnostic::new(format!("invalid numeric argument for `{flag}`: `{value}`"), 1)
        })
    }

    fn take_type_filter(&mut self) -> Result<FileTypeFilter, Diagnostic> {
        match self.take_string("-type")?.as_str() {
            "f" => Ok(FileTypeFilter::File),
            "d" => Ok(FileTypeFilter::Directory),
            "l" => Ok(FileTypeFilter::Symlink),
            "b" => Ok(FileTypeFilter::Block),
            "c" => Ok(FileTypeFilter::Character),
            "p" => Ok(FileTypeFilter::Fifo),
            "s" => Ok(FileTypeFilter::Socket),
            other => Err(Diagnostic::new(
                format!("unsupported -type value `{other}`"),
                1,
            )),
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
                .ok_or_else(|| Diagnostic::new("unterminated exec-style action", 1))?;

            match token.as_str() {
                ";" => break,
                "+" => {
                    batch = true;
                    break;
                }
                _ => argv.push(token),
            }
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
            self.peek().as_deref(),
            Some(token) if token != ")" && token != "-o" && token != "-or"
        )
    }
}
