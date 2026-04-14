use crate::ast::{Action, CommandAst, Expr, FileTypeFilter, Predicate};
use crate::diagnostics::Diagnostic;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub start_paths: Vec<PathBuf>,
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
    Type(FileTypeFilter),
    True,
    False,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputAction {
    Print,
    Print0,
}

pub fn plan_command(ast: CommandAst, workers: usize) -> Result<ExecutionPlan, Diagnostic> {
    let CommandAst { start_paths, expr } = ast;
    let mut traversal = TraversalOptions {
        min_depth: 0,
        max_depth: None,
    };
    let mut saw_output = false;
    let lowered = lower_expr(expr, &mut traversal, &mut saw_output)?;

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
        traversal,
        expr,
        mode,
    })
}

fn lower_expr(
    expr: Expr,
    traversal: &mut TraversalOptions,
    saw_output: &mut bool,
) -> Result<RuntimeExpr, Diagnostic> {
    match expr {
        Expr::And(items) => {
            let mut lowered = Vec::with_capacity(items.len());
            for item in items {
                lowered.push(lower_expr(item, traversal, saw_output)?);
            }
            Ok(RuntimeExpr::And(lowered))
        }
        Expr::Or(left, right) => Ok(RuntimeExpr::Or(
            Box::new(lower_expr(*left, traversal, saw_output)?),
            Box::new(lower_expr(*right, traversal, saw_output)?),
        )),
        Expr::Not(inner) => Ok(RuntimeExpr::Not(Box::new(lower_expr(
            *inner, traversal, saw_output,
        )?))),
        Expr::Predicate(predicate) => lower_predicate(predicate, traversal),
        Expr::Action(action) => lower_action(action, saw_output),
    }
}

fn lower_predicate(
    predicate: Predicate,
    traversal: &mut TraversalOptions,
) -> Result<RuntimeExpr, Diagnostic> {
    match predicate {
        Predicate::MaxDepth(value) => {
            traversal.max_depth = Some(value as usize);
            Ok(RuntimeExpr::Predicate(RuntimePredicate::True))
        }
        Predicate::MinDepth(value) => {
            traversal.min_depth = value as usize;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::True))
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
        Predicate::Type(kind) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Type(kind))),
        Predicate::True => Ok(RuntimeExpr::Predicate(RuntimePredicate::True)),
        Predicate::False => Ok(RuntimeExpr::Predicate(RuntimePredicate::False)),
    }
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
