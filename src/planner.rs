use crate::account::{resolve_group_id, resolve_user_id};
use crate::ast::{Action, CommandAst, Expr, FileTypeFilter, GlobalOption, Predicate};
use crate::diagnostics::Diagnostic;
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::numeric::{parse_numeric_argument, NumericComparison};
use crate::optimizer::optimize_read_only_and_chains;
use crate::perm::{parse_perm_argument, PermMatcher};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub start_paths: Vec<PathBuf>,
    pub follow_mode: FollowMode,
    pub traversal: TraversalOptions,
    pub expr: RuntimeExpr,
    pub mode: ExecutionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraversalOptions {
    pub min_depth: usize,
    pub max_depth: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    OrderedSingle,
    ParallelRelaxed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeExpr {
    And(Vec<RuntimeExpr>),
    Or(Box<RuntimeExpr>, Box<RuntimeExpr>),
    Not(Box<RuntimeExpr>),
    Predicate(RuntimePredicate),
    Action(OutputAction),
    TraversalBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePredicate {
    Name {
        pattern: OsString,
        case_insensitive: bool,
    },
    Path {
        pattern: OsString,
        case_insensitive: bool,
    },
    Inum(NumericComparison),
    Links(NumericComparison),
    SameFile(FileIdentity),
    LName {
        pattern: OsString,
        case_insensitive: bool,
    },
    Uid(NumericComparison),
    Gid(NumericComparison),
    User(u32),
    Group(u32),
    NoUser,
    NoGroup,
    Perm(PermMatcher),
    Type(FileTypeFilter),
    XType(FileTypeFilter),
    True,
    False,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputAction {
    Print,
    Print0,
}

pub fn plan_command(ast: CommandAst, workers: usize) -> Result<ExecutionPlan, Diagnostic> {
    let follow_mode = resolve_follow_mode(&ast.global_options);
    let CommandAst {
        start_paths, expr, ..
    } = ast;
    let mut traversal = TraversalOptions {
        min_depth: 0,
        max_depth: None,
    };
    let mut saw_output = false;
    let lowered = lower_expr(expr, &mut traversal, &mut saw_output, follow_mode)?;
    let lowered = optimize_read_only_and_chains(lowered);

    let expr = if saw_output {
        lowered
    } else {
        RuntimeExpr::And(vec![lowered, RuntimeExpr::Action(OutputAction::Print)])
    };

    let mode = if workers <= 1 {
        ExecutionMode::OrderedSingle
    } else {
        ExecutionMode::ParallelRelaxed
    };

    Ok(ExecutionPlan {
        start_paths,
        follow_mode,
        traversal,
        expr,
        mode,
    })
}

fn resolve_follow_mode(global_options: &[GlobalOption]) -> FollowMode {
    global_options
        .iter()
        .fold(FollowMode::Physical, |_, option| match option {
            GlobalOption::Follow(next) => *next,
        })
}

fn lower_expr(
    expr: Expr,
    traversal: &mut TraversalOptions,
    saw_output: &mut bool,
    follow_mode: FollowMode,
) -> Result<RuntimeExpr, Diagnostic> {
    match expr {
        Expr::And(items) => {
            let mut lowered = Vec::with_capacity(items.len());
            for item in items {
                lowered.push(lower_expr(item, traversal, saw_output, follow_mode)?);
            }
            Ok(RuntimeExpr::And(lowered))
        }
        Expr::Or(left, right) => Ok(RuntimeExpr::Or(
            Box::new(lower_expr(*left, traversal, saw_output, follow_mode)?),
            Box::new(lower_expr(*right, traversal, saw_output, follow_mode)?),
        )),
        Expr::Not(inner) => Ok(RuntimeExpr::Not(Box::new(lower_expr(
            *inner,
            traversal,
            saw_output,
            follow_mode,
        )?))),
        Expr::Predicate(predicate) => lower_predicate(predicate, traversal, follow_mode),
        Expr::Action(action) => lower_action(action, saw_output),
    }
}

fn lower_predicate(
    predicate: Predicate,
    traversal: &mut TraversalOptions,
    follow_mode: FollowMode,
) -> Result<RuntimeExpr, Diagnostic> {
    match predicate {
        Predicate::MaxDepth(value) => {
            traversal.max_depth = Some(value as usize);
            Ok(RuntimeExpr::TraversalBoundary)
        }
        Predicate::MinDepth(value) => {
            traversal.min_depth = value as usize;
            Ok(RuntimeExpr::TraversalBoundary)
        }
        Predicate::Name {
            pattern,
            case_insensitive,
        } => Ok(RuntimeExpr::Predicate(RuntimePredicate::Name {
            pattern,
            case_insensitive,
        })),
        Predicate::Path {
            pattern,
            case_insensitive,
        } => Ok(RuntimeExpr::Predicate(RuntimePredicate::Path {
            pattern,
            case_insensitive,
        })),
        Predicate::Inum(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Inum(
            parse_numeric_argument("-inum", raw.as_os_str())?,
        ))),
        Predicate::Links(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Links(
            parse_numeric_argument("-links", raw.as_os_str())?,
        ))),
        Predicate::SameFile(path) => Ok(RuntimeExpr::Predicate(RuntimePredicate::SameFile(
            resolve_samefile_reference(&path, follow_mode)?,
        ))),
        Predicate::LName {
            pattern,
            case_insensitive,
        } => Ok(RuntimeExpr::Predicate(RuntimePredicate::LName {
            pattern,
            case_insensitive,
        })),
        Predicate::Uid(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Uid(
            parse_numeric_argument("-uid", raw.as_os_str())?,
        ))),
        Predicate::Gid(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Gid(
            parse_numeric_argument("-gid", raw.as_os_str())?,
        ))),
        Predicate::User(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::User(
            resolve_user_id(raw.as_os_str())?,
        ))),
        Predicate::Group(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Group(
            resolve_group_id(raw.as_os_str())?,
        ))),
        Predicate::NoUser => Ok(RuntimeExpr::Predicate(RuntimePredicate::NoUser)),
        Predicate::NoGroup => Ok(RuntimeExpr::Predicate(RuntimePredicate::NoGroup)),
        Predicate::Perm(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Perm(
            parse_perm_argument(raw.as_os_str())?,
        ))),
        Predicate::Size(_) => Err(stage8_planner_unimplemented("-size")),
        Predicate::ATime(_) => Err(stage8_planner_unimplemented("-atime")),
        Predicate::CTime(_) => Err(stage8_planner_unimplemented("-ctime")),
        Predicate::MTime(_) => Err(stage8_planner_unimplemented("-mtime")),
        Predicate::AMin(_) => Err(stage8_planner_unimplemented("-amin")),
        Predicate::CMin(_) => Err(stage8_planner_unimplemented("-cmin")),
        Predicate::MMin(_) => Err(stage8_planner_unimplemented("-mmin")),
        Predicate::Newer(_) => Err(stage8_planner_unimplemented("-newer")),
        Predicate::ANewer(_) => Err(stage8_planner_unimplemented("-anewer")),
        Predicate::CNewer(_) => Err(stage8_planner_unimplemented("-cnewer")),
        Predicate::NewerXY { .. } => Err(stage8_planner_unimplemented("-newerXY")),
        Predicate::DayStart => Err(stage8_planner_unimplemented("-daystart")),
        Predicate::Type(kind) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Type(kind))),
        Predicate::XType(kind) => Ok(RuntimeExpr::Predicate(RuntimePredicate::XType(kind))),
        Predicate::True => Ok(RuntimeExpr::Predicate(RuntimePredicate::True)),
        Predicate::False => Ok(RuntimeExpr::Predicate(RuntimePredicate::False)),
    }
}

fn stage8_planner_unimplemented(flag: &str) -> Diagnostic {
    Diagnostic::unsupported(format!(
        "unsupported until stage 8 planner implementation: {flag}"
    ))
}

fn resolve_samefile_reference(
    path: &Path,
    follow_mode: FollowMode,
) -> Result<FileIdentity, Diagnostic> {
    let metadata = match follow_mode {
        FollowMode::Physical => fs::symlink_metadata(path),
        FollowMode::CommandLineOnly | FollowMode::Logical => {
            fs::metadata(path).or_else(|_| fs::symlink_metadata(path))
        }
    }
    .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))?;

    Ok(FileIdentity::from_metadata(&metadata))
}

fn lower_action(action: Action, saw_output: &mut bool) -> Result<RuntimeExpr, Diagnostic> {
    match action {
        Action::Print => {
            *saw_output = true;
            Ok(RuntimeExpr::Action(OutputAction::Print))
        }
        Action::Print0 => {
            *saw_output = true;
            Ok(RuntimeExpr::Action(OutputAction::Print0))
        }
        Action::Exec { .. } => Err(Diagnostic::unsupported(
            "unsupported in read-only v0: -exec",
        )),
        Action::ExecDir { .. } => Err(Diagnostic::unsupported(
            "unsupported in read-only v0: -execdir",
        )),
        Action::Ok { .. } => Err(Diagnostic::unsupported("unsupported in read-only v0: -ok")),
        Action::OkDir { .. } => Err(Diagnostic::unsupported(
            "unsupported in read-only v0: -okdir",
        )),
        Action::Delete => Err(Diagnostic::unsupported(
            "unsupported in read-only v0: -delete",
        )),
    }
}
