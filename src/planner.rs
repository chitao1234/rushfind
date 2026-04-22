use crate::account::{PrincipalId, resolve_group_principal, resolve_user_principal};
use crate::ast::{Action, CommandAst, Expr, FileTypeFilter, GlobalOption, Predicate};
use crate::diagnostics::Diagnostic;
use crate::exec::{
    BatchedExecAction, ExecBatchId, ExecSemantics, ImmediateExecAction, compile_batched_exec,
    compile_immediate_exec,
};
use crate::file_output::{FileOutputId, FileOutputTerminator, PlannedFileOutput};
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::numeric::{NumericComparison, parse_numeric_argument};
use crate::optimizer::optimize_read_only_and_chains;
use crate::pattern::{CompiledGlob, GlobCaseMode, GlobSlashMode};
use crate::perm::{PermMatcher, parse_perm_argument};
use crate::platform::{PlatformCapabilities, PlatformFeature, SupportLevel, active_capabilities};
use crate::printf::{PrintfAtom, PrintfDirectiveKind, PrintfProgram, compile_printf_program};
use crate::regex_match::{RegexDialect, RegexMatcher};
use crate::runtime_policy::{RuntimePolicy, build_traversal_control_plan};
use crate::size::{SizeMatcher, parse_size_argument};
use crate::time::{
    NewerMatcher, RelativeTimeMatcher, RelativeTimeUnit, Timestamp, TimestampKind, UsedMatcher,
    local_day_start, parse_relative_time_argument, parse_time_comparison,
    resolve_reference_matcher,
};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParallelExecutionPolicy {
    PreOrderFastPath,
    PostOrderSubtree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActionProfile {
    pub has_local_immediate: bool,
    pub has_local_batched: bool,
    pub has_global_control: bool,
    pub has_subtree_finalizer: bool,
    pub has_ordered_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub start_paths: Vec<PathBuf>,
    pub follow_mode: FollowMode,
    pub traversal: TraversalOptions,
    pub runtime: RuntimeRequirements,
    pub startup_warnings: Vec<String>,
    pub file_outputs: Vec<PlannedFileOutput>,
    pub expr: RuntimeExpr,
    pub mode: ExecutionMode,
    pub parallel_policy: Option<ParallelExecutionPolicy>,
    pub action_profile: ActionProfile,
    pub(crate) runtime_policy: RuntimePolicy,
    pub(crate) traversal_control: Option<RuntimeExpr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeRequirements {
    pub mount_snapshot: bool,
    pub evaluation_now: Timestamp,
    pub execdir_requires_safe_path: bool,
    pub messages_locale_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraversalOptions {
    pub min_depth: usize,
    pub max_depth: Option<usize>,
    pub same_file_system: bool,
    pub order: TraversalOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalOrder {
    PreOrder,
    DepthFirstPostOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    OrderedSingle,
    ParallelRelaxed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeExpr {
    And(Arc<[RuntimeExpr]>),
    Or(Arc<RuntimeExpr>, Arc<RuntimeExpr>),
    Not(Arc<RuntimeExpr>),
    Predicate(RuntimePredicate),
    Action(RuntimeAction),
    Barrier,
}

impl RuntimeExpr {
    pub fn and(items: Vec<Self>) -> Self {
        Self::And(items.into())
    }

    pub fn or(left: Self, right: Self) -> Self {
        Self::Or(Arc::new(left), Arc::new(right))
    }

    pub fn negate(inner: Self) -> Self {
        Self::Not(Arc::new(inner))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePredicate {
    Prune,
    FsType(OsString),
    Readable,
    Writable,
    Executable,
    Name(CompiledGlob),
    Path(CompiledGlob),
    Regex(RegexMatcher),
    Inum(NumericComparison),
    Links(NumericComparison),
    SameFile(FileIdentity),
    LName(CompiledGlob),
    Uid(NumericComparison),
    Gid(NumericComparison),
    User(PrincipalId),
    Group(PrincipalId),
    NoUser,
    NoGroup,
    Perm(PermMatcher),
    Size(SizeMatcher),
    Empty,
    Used(UsedMatcher),
    RelativeTime(RelativeTimeMatcher),
    Newer(NewerMatcher),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeAction {
    Output(OutputAction),
    Printf(PrintfProgram),
    FilePrint {
        destination: FileOutputId,
        terminator: FileOutputTerminator,
    },
    FilePrintf {
        destination: FileOutputId,
        program: PrintfProgram,
    },
    Ls,
    FileLs {
        destination: FileOutputId,
    },
    Quit,
    ExecImmediate(ImmediateExecAction),
    ExecBatched(BatchedExecAction),
    ExecPrompt(ImmediateExecAction),
    Delete,
}

fn compile_glob(
    flag: &'static str,
    pattern: &std::ffi::OsStr,
    case_insensitive: bool,
    slash_mode: GlobSlashMode,
) -> Result<CompiledGlob, Diagnostic> {
    CompiledGlob::compile(
        flag,
        pattern,
        if case_insensitive {
            GlobCaseMode::Insensitive
        } else {
            GlobCaseMode::Sensitive
        },
        slash_mode,
    )
}

fn normalize_match_pattern(pattern: &std::ffi::OsStr) -> std::borrow::Cow<'_, std::ffi::OsStr> {
    crate::platform::path::normalize_match_text(pattern)
}

pub fn plan_command(ast: CommandAst, workers: usize) -> Result<ExecutionPlan, Diagnostic> {
    plan_command_with_now(
        ast,
        workers,
        Timestamp::from_system_time(SystemTime::now())?,
    )
}

pub fn plan_command_with_now(
    ast: CommandAst,
    workers: usize,
    now: Timestamp,
) -> Result<ExecutionPlan, Diagnostic> {
    plan_command_with_now_and_capabilities(ast, workers, now, active_capabilities())
}

pub(crate) fn plan_command_with_now_and_capabilities(
    ast: CommandAst,
    workers: usize,
    now: Timestamp,
    capabilities: &PlatformCapabilities,
) -> Result<ExecutionPlan, Diagnostic> {
    let follow_mode = resolve_follow_mode(&ast.global_options);
    let CommandAst {
        start_paths, expr, ..
    } = ast;
    let mut traversal = TraversalOptions {
        min_depth: 0,
        max_depth: None,
        same_file_system: false,
        order: TraversalOrder::PreOrder,
    };
    let mut runtime = RuntimeRequirements {
        mount_snapshot: false,
        evaluation_now: now,
        execdir_requires_safe_path: false,
        messages_locale_required: false,
    };
    let mut state = PlanningState::new(now);
    let lowered = lower_expr(
        expr,
        &mut traversal,
        &mut runtime,
        &mut state,
        follow_mode,
        capabilities,
    )?;
    let lowered = optimize_read_only_and_chains(lowered);

    if state.saw_delete {
        traversal.order = TraversalOrder::DepthFirstPostOrder;
    }

    let expr = if state.saw_action {
        lowered
    } else {
        RuntimeExpr::and(vec![
            lowered,
            RuntimeExpr::Action(RuntimeAction::Output(OutputAction::Print)),
        ])
    };
    let action_profile = compute_action_profile(&expr);

    let mode = if workers <= 1 {
        ExecutionMode::OrderedSingle
    } else {
        ExecutionMode::ParallelRelaxed
    };
    let parallel_policy = choose_parallel_policy(workers, traversal, action_profile);
    let runtime_policy = RuntimePolicy::derive(
        workers,
        traversal.order,
        mode == ExecutionMode::OrderedSingle,
    );
    let traversal_control = build_traversal_control_plan(&expr, traversal.order);

    Ok(ExecutionPlan {
        start_paths,
        follow_mode,
        traversal,
        runtime,
        startup_warnings: state.startup_warnings.clone(),
        file_outputs: state.file_outputs.clone(),
        expr,
        mode,
        parallel_policy,
        action_profile,
        runtime_policy,
        traversal_control,
    })
}

fn choose_parallel_policy(
    workers: usize,
    traversal: TraversalOptions,
    action_profile: ActionProfile,
) -> Option<ParallelExecutionPolicy> {
    if workers <= 1 {
        return None;
    }

    if traversal.order == TraversalOrder::DepthFirstPostOrder
        || action_profile.has_subtree_finalizer
    {
        Some(ParallelExecutionPolicy::PostOrderSubtree)
    } else {
        Some(ParallelExecutionPolicy::PreOrderFastPath)
    }
}

fn compute_action_profile(expr: &RuntimeExpr) -> ActionProfile {
    let mut profile = ActionProfile::default();
    populate_action_profile(expr, &mut profile);
    profile
}

fn populate_action_profile(expr: &RuntimeExpr, profile: &mut ActionProfile) {
    match expr {
        RuntimeExpr::And(items) => {
            for item in items.iter() {
                populate_action_profile(item, profile);
            }
        }
        RuntimeExpr::Or(left, right) => {
            populate_action_profile(left, profile);
            populate_action_profile(right, profile);
        }
        RuntimeExpr::Not(inner) => populate_action_profile(inner, profile),
        RuntimeExpr::Action(action) => match action {
            RuntimeAction::Output(_)
            | RuntimeAction::Printf(_)
            | RuntimeAction::FilePrint { .. }
            | RuntimeAction::FilePrintf { .. }
            | RuntimeAction::Ls
            | RuntimeAction::FileLs { .. } => {}
            RuntimeAction::Quit => profile.has_global_control = true,
            RuntimeAction::ExecImmediate(_) | RuntimeAction::ExecPrompt(_) => {
                profile.has_local_immediate = true;
            }
            RuntimeAction::ExecBatched(_) => profile.has_local_batched = true,
            RuntimeAction::Delete => profile.has_subtree_finalizer = true,
        },
        RuntimeExpr::Predicate(_) | RuntimeExpr::Barrier => {}
    }
}

fn resolve_follow_mode(global_options: &[GlobalOption]) -> FollowMode {
    global_options
        .iter()
        .fold(FollowMode::Physical, |_, option| match option {
            GlobalOption::Follow(next) => *next,
        })
}

fn require_platform_feature(
    capabilities: &PlatformCapabilities,
    feature: PlatformFeature,
    state: &mut PlanningState,
) -> Result<(), Diagnostic> {
    match capabilities.support(feature) {
        SupportLevel::Exact => Ok(()),
        SupportLevel::Approximate(message) => {
            state
                .startup_warnings
                .push(format!("rfd: warning: {message}"));
            Ok(())
        }
        SupportLevel::Unsupported(message) => Err(Diagnostic::unsupported(message)),
    }
}

fn validate_platform_printf_program(
    program: &PrintfProgram,
    capabilities: &PlatformCapabilities,
    state: &mut PlanningState,
) -> Result<(), Diagnostic> {
    for atom in &program.atoms {
        let PrintfAtom::Directive(directive) = atom else {
            continue;
        };
        match directive.kind {
            PrintfDirectiveKind::UserId | PrintfDirectiveKind::GroupId => {
                require_platform_feature(capabilities, PlatformFeature::NumericOwnership, state)?;
            }
            PrintfDirectiveKind::ModeOctal | PrintfDirectiveKind::ModeSymbolic => {
                require_platform_feature(capabilities, PlatformFeature::ModeBits, state)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn lower_expr(
    expr: Expr,
    traversal: &mut TraversalOptions,
    runtime: &mut RuntimeRequirements,
    state: &mut PlanningState,
    follow_mode: FollowMode,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    match expr {
        Expr::And(items) => {
            let mut lowered = Vec::with_capacity(items.len());
            for item in items {
                lowered.push(lower_expr(
                    item,
                    traversal,
                    runtime,
                    state,
                    follow_mode,
                    capabilities,
                )?);
            }
            Ok(RuntimeExpr::and(lowered))
        }
        Expr::Or(left, right) => Ok(RuntimeExpr::or(
            lower_expr(*left, traversal, runtime, state, follow_mode, capabilities)?,
            lower_expr(*right, traversal, runtime, state, follow_mode, capabilities)?,
        )),
        Expr::Not(inner) => Ok(RuntimeExpr::negate(lower_expr(
            *inner,
            traversal,
            runtime,
            state,
            follow_mode,
            capabilities,
        )?)),
        Expr::Predicate(predicate) => lower_predicate(
            predicate,
            traversal,
            runtime,
            state,
            follow_mode,
            capabilities,
        ),
        Expr::Action(action) => lower_action(action, runtime, state, capabilities),
    }
}

fn lower_predicate(
    predicate: Predicate,
    traversal: &mut TraversalOptions,
    runtime: &mut RuntimeRequirements,
    state: &mut PlanningState,
    follow_mode: FollowMode,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    match predicate {
        Predicate::MaxDepth(value) => {
            traversal.max_depth = Some(value as usize);
            Ok(RuntimeExpr::Barrier)
        }
        Predicate::MinDepth(value) => {
            traversal.min_depth = value as usize;
            Ok(RuntimeExpr::Barrier)
        }
        Predicate::Depth => {
            traversal.order = TraversalOrder::DepthFirstPostOrder;
            Ok(RuntimeExpr::Barrier)
        }
        Predicate::Prune => Ok(RuntimeExpr::Predicate(RuntimePredicate::Prune)),
        Predicate::XDev => {
            require_platform_feature(capabilities, PlatformFeature::SameFileSystem, state)?;
            traversal.same_file_system = true;
            Ok(RuntimeExpr::Barrier)
        }
        Predicate::Readable => {
            require_platform_feature(capabilities, PlatformFeature::AccessPredicates, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Readable))
        }
        Predicate::Writable => {
            require_platform_feature(capabilities, PlatformFeature::AccessPredicates, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Writable))
        }
        Predicate::Executable => {
            require_platform_feature(capabilities, PlatformFeature::AccessPredicates, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Executable))
        }
        Predicate::Name {
            pattern,
            case_insensitive,
        } => {
            if case_insensitive {
                require_platform_feature(
                    capabilities,
                    PlatformFeature::CaseInsensitiveGlob,
                    state,
                )?;
            }
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Name(
                compile_glob(
                    if case_insensitive { "-iname" } else { "-name" },
                    pattern.as_os_str(),
                    case_insensitive,
                    GlobSlashMode::Literal,
                )?,
            )))
        }
        Predicate::Path {
            pattern,
            case_insensitive,
        } => {
            if case_insensitive {
                require_platform_feature(
                    capabilities,
                    PlatformFeature::CaseInsensitiveGlob,
                    state,
                )?;
            }
            let normalized_pattern = normalize_match_pattern(pattern.as_os_str());
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Path(
                compile_glob(
                    if case_insensitive { "-ipath" } else { "-path" },
                    normalized_pattern.as_ref(),
                    case_insensitive,
                    GlobSlashMode::Literal,
                )?,
            )))
        }
        Predicate::Regex {
            pattern,
            case_insensitive,
        } => Ok(RuntimeExpr::Predicate(RuntimePredicate::Regex(
            RegexMatcher::compile(
                if case_insensitive {
                    "-iregex"
                } else {
                    "-regex"
                },
                state.regex_dialect,
                pattern.as_os_str(),
                case_insensitive,
            )?,
        ))),
        Predicate::RegexType(raw) => {
            state.regex_dialect = RegexDialect::parse(raw.as_os_str())?;
            Ok(RuntimeExpr::Barrier)
        }
        Predicate::FsType(type_name) => {
            require_platform_feature(capabilities, PlatformFeature::FsType, state)?;
            runtime.mount_snapshot = true;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::FsType(type_name)))
        }
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
        } => {
            if case_insensitive {
                require_platform_feature(
                    capabilities,
                    PlatformFeature::CaseInsensitiveGlob,
                    state,
                )?;
            }
            let normalized_pattern = normalize_match_pattern(pattern.as_os_str());
            Ok(RuntimeExpr::Predicate(RuntimePredicate::LName(
                compile_glob(
                    if case_insensitive {
                        "-ilname"
                    } else {
                        "-lname"
                    },
                    normalized_pattern.as_ref(),
                    case_insensitive,
                    GlobSlashMode::Literal,
                )?,
            )))
        }
        Predicate::Uid(raw) => {
            require_platform_feature(capabilities, PlatformFeature::NumericOwnership, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Uid(
                parse_numeric_argument("-uid", raw.as_os_str())?,
            )))
        }
        Predicate::Gid(raw) => {
            require_platform_feature(capabilities, PlatformFeature::NumericOwnership, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Gid(
                parse_numeric_argument("-gid", raw.as_os_str())?,
            )))
        }
        Predicate::User(raw) => {
            require_platform_feature(capabilities, PlatformFeature::NamedOwnership, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::User(
                resolve_user_principal(raw.as_os_str())?,
            )))
        }
        Predicate::Group(raw) => {
            require_platform_feature(capabilities, PlatformFeature::NamedOwnership, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Group(
                resolve_group_principal(raw.as_os_str())?,
            )))
        }
        Predicate::NoUser => {
            require_platform_feature(capabilities, PlatformFeature::NamedOwnership, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::NoUser))
        }
        Predicate::NoGroup => {
            require_platform_feature(capabilities, PlatformFeature::NamedOwnership, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::NoGroup))
        }
        Predicate::Perm(raw) => {
            require_platform_feature(capabilities, PlatformFeature::ModeBits, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Perm(
                parse_perm_argument(raw.as_os_str())?,
            )))
        }
        Predicate::Size(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Size(
            parse_size_argument(raw.as_os_str())?,
        ))),
        Predicate::Empty => Ok(RuntimeExpr::Predicate(RuntimePredicate::Empty)),
        Predicate::Used(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Used(
            UsedMatcher {
                comparison: parse_time_comparison("-used", raw.as_os_str())?,
            },
        ))),
        Predicate::ATime(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::RelativeTime(
            parse_relative_time_argument(
                "-atime",
                raw.as_os_str(),
                TimestampKind::Access,
                RelativeTimeUnit::Days,
                state.temporal.relative_baseline()?,
                state.temporal.daystart_active,
            )?,
        ))),
        Predicate::CTime(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::RelativeTime(
            parse_relative_time_argument(
                "-ctime",
                raw.as_os_str(),
                TimestampKind::Change,
                RelativeTimeUnit::Days,
                state.temporal.relative_baseline()?,
                state.temporal.daystart_active,
            )?,
        ))),
        Predicate::MTime(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::RelativeTime(
            parse_relative_time_argument(
                "-mtime",
                raw.as_os_str(),
                TimestampKind::Modification,
                RelativeTimeUnit::Days,
                state.temporal.relative_baseline()?,
                state.temporal.daystart_active,
            )?,
        ))),
        Predicate::AMin(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::RelativeTime(
            parse_relative_time_argument(
                "-amin",
                raw.as_os_str(),
                TimestampKind::Access,
                RelativeTimeUnit::Minutes,
                state.temporal.relative_baseline()?,
                state.temporal.daystart_active,
            )?,
        ))),
        Predicate::CMin(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::RelativeTime(
            parse_relative_time_argument(
                "-cmin",
                raw.as_os_str(),
                TimestampKind::Change,
                RelativeTimeUnit::Minutes,
                state.temporal.relative_baseline()?,
                state.temporal.daystart_active,
            )?,
        ))),
        Predicate::MMin(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::RelativeTime(
            parse_relative_time_argument(
                "-mmin",
                raw.as_os_str(),
                TimestampKind::Modification,
                RelativeTimeUnit::Minutes,
                state.temporal.relative_baseline()?,
                state.temporal.daystart_active,
            )?,
        ))),
        Predicate::Newer(path) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Newer(
            resolve_reference_matcher("-newer", 'm', 'm', path.as_os_str(), follow_mode)?,
        ))),
        Predicate::ANewer(path) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Newer(
            resolve_reference_matcher("-anewer", 'a', 'm', path.as_os_str(), follow_mode)?,
        ))),
        Predicate::CNewer(path) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Newer(
            resolve_reference_matcher("-cnewer", 'c', 'm', path.as_os_str(), follow_mode)?,
        ))),
        Predicate::NewerXY {
            current,
            reference,
            reference_arg,
        } => {
            if current == 'B' || reference == 'B' {
                require_platform_feature(capabilities, PlatformFeature::BirthTime, state)?;
            }
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Newer(
                resolve_reference_matcher(
                    "-newerXY",
                    current,
                    reference,
                    reference_arg.as_os_str(),
                    follow_mode,
                )?,
            )))
        }
        Predicate::DayStart => {
            state.temporal.daystart_active = true;
            Ok(RuntimeExpr::Barrier)
        }
        Predicate::Type(kind) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Type(kind))),
        Predicate::XType(kind) => Ok(RuntimeExpr::Predicate(RuntimePredicate::XType(kind))),
        Predicate::True => Ok(RuntimeExpr::Predicate(RuntimePredicate::True)),
        Predicate::False => Ok(RuntimeExpr::Predicate(RuntimePredicate::False)),
    }
}

#[derive(Debug, Clone, Copy)]
struct TemporalPlanningState {
    now: Timestamp,
    daystart_active: bool,
}

#[derive(Debug, Clone)]
struct PlanningState {
    temporal: TemporalPlanningState,
    regex_dialect: RegexDialect,
    saw_action: bool,
    saw_delete: bool,
    next_exec_batch_id: ExecBatchId,
    startup_warnings: Vec<String>,
    file_outputs: Vec<PlannedFileOutput>,
    file_output_ids: BTreeMap<PathBuf, FileOutputId>,
}

impl TemporalPlanningState {
    fn relative_baseline(&self) -> Result<Timestamp, Diagnostic> {
        if self.daystart_active {
            local_day_start(self.now)
        } else {
            Ok(self.now)
        }
    }
}

impl PlanningState {
    fn new(now: Timestamp) -> Self {
        Self {
            temporal: TemporalPlanningState {
                now,
                daystart_active: false,
            },
            regex_dialect: RegexDialect::Emacs,
            saw_action: false,
            saw_delete: false,
            next_exec_batch_id: 0,
            startup_warnings: Vec::new(),
            file_outputs: Vec::new(),
            file_output_ids: BTreeMap::new(),
        }
    }
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

fn register_file_output(state: &mut PlanningState, path: PathBuf) -> FileOutputId {
    if let Some(existing) = state.file_output_ids.get(&path) {
        return *existing;
    }

    let id = state.file_outputs.len();
    state.file_outputs.push(PlannedFileOutput {
        id,
        path: path.clone(),
    });
    state.file_output_ids.insert(path, id);
    id
}

fn lower_action(
    action: Action,
    runtime: &mut RuntimeRequirements,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    state.saw_action = true;

    match action {
        Action::Print => Ok(RuntimeExpr::Action(RuntimeAction::Output(
            OutputAction::Print,
        ))),
        Action::Print0 => Ok(RuntimeExpr::Action(RuntimeAction::Output(
            OutputAction::Print0,
        ))),
        Action::Printf { format } => {
            let compiled = compile_printf_program("-printf", format.as_os_str())?;
            if compiled.program.requires_mount_snapshot() {
                require_platform_feature(capabilities, PlatformFeature::FsType, state)?;
                runtime.mount_snapshot = true;
            }
            validate_platform_printf_program(&compiled.program, capabilities, state)?;
            state.startup_warnings.extend(compiled.warnings);
            Ok(RuntimeExpr::Action(RuntimeAction::Printf(compiled.program)))
        }
        Action::FPrint { path } => Ok(RuntimeExpr::Action(RuntimeAction::FilePrint {
            destination: register_file_output(state, path),
            terminator: FileOutputTerminator::Newline,
        })),
        Action::FPrint0 { path } => Ok(RuntimeExpr::Action(RuntimeAction::FilePrint {
            destination: register_file_output(state, path),
            terminator: FileOutputTerminator::Nul,
        })),
        Action::FPrintf { path, format } => {
            let compiled = compile_printf_program("-fprintf", format.as_os_str())?;
            if compiled.program.requires_mount_snapshot() {
                require_platform_feature(capabilities, PlatformFeature::FsType, state)?;
                runtime.mount_snapshot = true;
            }
            validate_platform_printf_program(&compiled.program, capabilities, state)?;
            state.startup_warnings.extend(compiled.warnings);
            Ok(RuntimeExpr::Action(RuntimeAction::FilePrintf {
                destination: register_file_output(state, path),
                program: compiled.program,
            }))
        }
        Action::Ls => {
            require_platform_feature(capabilities, PlatformFeature::ModeBits, state)?;
            Ok(RuntimeExpr::Action(RuntimeAction::Ls))
        }
        Action::Fls { path } => Ok(RuntimeExpr::Action(RuntimeAction::FileLs {
            destination: {
                require_platform_feature(capabilities, PlatformFeature::ModeBits, state)?;
                register_file_output(state, path)
            },
        })),
        Action::Quit => Ok(RuntimeExpr::Action(RuntimeAction::Quit)),
        Action::Exec { argv, batch: false } => Ok(RuntimeExpr::Action(
            RuntimeAction::ExecImmediate(compile_immediate_exec(ExecSemantics::Normal, &argv)),
        )),
        Action::Exec { argv, batch: true } => {
            let id = state.next_exec_batch_id;
            state.next_exec_batch_id += 1;
            Ok(RuntimeExpr::Action(RuntimeAction::ExecBatched(
                compile_batched_exec(id, ExecSemantics::Normal, &argv)?,
            )))
        }
        Action::ExecDir { argv, batch: false } => {
            runtime.execdir_requires_safe_path = true;
            Ok(RuntimeExpr::Action(RuntimeAction::ExecImmediate(
                compile_immediate_exec(ExecSemantics::DirLocal, &argv),
            )))
        }
        Action::ExecDir { argv, batch: true } => {
            runtime.execdir_requires_safe_path = true;
            let id = state.next_exec_batch_id;
            state.next_exec_batch_id += 1;
            Ok(RuntimeExpr::Action(RuntimeAction::ExecBatched(
                compile_batched_exec(id, ExecSemantics::DirLocal, &argv)?,
            )))
        }
        Action::Ok { argv, batch: false } => {
            require_platform_feature(capabilities, PlatformFeature::MessagesLocale, state)?;
            runtime.messages_locale_required = true;
            Ok(RuntimeExpr::Action(RuntimeAction::ExecPrompt(
                compile_immediate_exec(ExecSemantics::Normal, &argv),
            )))
        }
        Action::Ok { batch: true, .. } => {
            Err(Diagnostic::parse("`-ok` only supports the `;` terminator"))
        }
        Action::OkDir { argv, batch: false } => {
            require_platform_feature(capabilities, PlatformFeature::MessagesLocale, state)?;
            runtime.messages_locale_required = true;
            runtime.execdir_requires_safe_path = true;
            Ok(RuntimeExpr::Action(RuntimeAction::ExecPrompt(
                compile_immediate_exec(ExecSemantics::DirLocal, &argv),
            )))
        }
        Action::OkDir { batch: true, .. } => Err(Diagnostic::parse(
            "`-okdir` only supports the `;` terminator",
        )),
        Action::Delete => {
            state.saw_delete = true;
            Ok(RuntimeExpr::Action(RuntimeAction::Delete))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_command;
    use crate::platform::{PlatformCapabilities, PlatformFeature, SupportLevel};

    fn argv(items: &[&str]) -> Vec<std::ffi::OsString> {
        items.iter().map(|item| (*item).into()).collect()
    }

    fn linux_like_caps() -> PlatformCapabilities {
        PlatformCapabilities::for_tests()
            .with(PlatformFeature::FsType, SupportLevel::Exact)
            .with(PlatformFeature::SameFileSystem, SupportLevel::Exact)
            .with(PlatformFeature::BirthTime, SupportLevel::Exact)
            .with(PlatformFeature::NamedOwnership, SupportLevel::Exact)
            .with(PlatformFeature::NumericOwnership, SupportLevel::Exact)
            .with(PlatformFeature::AccessPredicates, SupportLevel::Exact)
            .with(PlatformFeature::MessagesLocale, SupportLevel::Exact)
            .with(PlatformFeature::CaseInsensitiveGlob, SupportLevel::Exact)
            .with(PlatformFeature::ModeBits, SupportLevel::Exact)
    }

    fn windows_like_caps() -> PlatformCapabilities {
        PlatformCapabilities::for_tests()
            .with(PlatformFeature::FsType, SupportLevel::Exact)
            .with(PlatformFeature::SameFileSystem, SupportLevel::Exact)
            .with(PlatformFeature::BirthTime, SupportLevel::Exact)
            .with(PlatformFeature::NamedOwnership, SupportLevel::Exact)
            .with(
                PlatformFeature::NumericOwnership,
                SupportLevel::Unsupported("numeric ownership is not supported on Windows"),
            )
            .with(PlatformFeature::AccessPredicates, SupportLevel::Exact)
            .with(
                PlatformFeature::MessagesLocale,
                SupportLevel::Approximate("interactive locale behavior is approximate on Windows"),
            )
            .with(PlatformFeature::CaseInsensitiveGlob, SupportLevel::Exact)
            .with(
                PlatformFeature::ModeBits,
                SupportLevel::Unsupported("Unix mode bits are not supported on Windows"),
            )
    }

    #[test]
    fn unsupported_platform_features_fail_during_planning() {
        let ast = parse_command(&argv(&[".", "-fstype", "tmpfs"])).unwrap();
        let caps = linux_like_caps().with(
            PlatformFeature::FsType,
            SupportLevel::Unsupported("`-fstype` is not supported on this platform"),
        );

        let error = plan_command_with_now_and_capabilities(ast, 1, Timestamp::new(0, 0), &caps)
            .unwrap_err();

        assert!(error.message.contains("-fstype"));
        assert!(error.message.contains("not supported"));
    }

    #[test]
    fn approximate_platform_features_emit_startup_warning() {
        let ast = parse_command(&argv(&[".", "-ok", "printf", "%s\\n", "{}", ";"])).unwrap();
        let caps = linux_like_caps().with(
            PlatformFeature::MessagesLocale,
            SupportLevel::Approximate(
                "interactive locale behavior is approximate on this platform",
            ),
        );

        let plan =
            plan_command_with_now_and_capabilities(ast, 1, Timestamp::new(0, 0), &caps).unwrap();

        assert!(plan.runtime.messages_locale_required);
        assert!(plan.startup_warnings.iter().any(|warning: &String| {
            warning.contains("interactive locale behavior is approximate")
        }));
    }

    #[test]
    fn exact_platform_features_keep_existing_runtime_bits() {
        let ast = parse_command(&argv(&[
            ".", "-fstype", "tmpfs", "-xdev", "-printf", "%F\\n",
        ]))
        .unwrap();
        let plan = plan_command_with_now_and_capabilities(
            ast,
            1,
            Timestamp::new(0, 0),
            &linux_like_caps(),
        )
        .unwrap();

        assert!(plan.runtime.mount_snapshot);
        assert!(plan.traversal.same_file_system);
        assert!(matches!(plan.expr, RuntimeExpr::And(_)));
    }

    #[test]
    fn windows_rejects_numeric_ownership_and_mode_printf_directives() {
        for (args, needle) in [
            (
                argv(&[".", "-printf", "%U\\n"]),
                "numeric ownership is not supported on Windows",
            ),
            (
                argv(&[".", "-printf", "%G\\n"]),
                "numeric ownership is not supported on Windows",
            ),
            (
                argv(&[".", "-printf", "%m\\n"]),
                "Unix mode bits are not supported on Windows",
            ),
            (
                argv(&[".", "-printf", "%M\\n"]),
                "Unix mode bits are not supported on Windows",
            ),
            (
                argv(&[".", "-fprintf", "out.txt", "%U\\n"]),
                "numeric ownership is not supported on Windows",
            ),
            (
                argv(&[".", "-fprintf", "out.txt", "%M\\n"]),
                "Unix mode bits are not supported on Windows",
            ),
        ] {
            let ast = parse_command(&args).unwrap();
            let error = plan_command_with_now_and_capabilities(
                ast,
                1,
                Timestamp::new(0, 0),
                &windows_like_caps(),
            )
            .unwrap_err();
            assert!(
                error.message.contains(needle),
                "{args:?} -> {}",
                error.message
            );
        }
    }

    #[test]
    fn windows_rejects_ls_until_a_windows_renderer_is_available() {
        for args in [argv(&[".", "-ls"]), argv(&[".", "-fls", "out.txt"])] {
            let ast = parse_command(&args).unwrap();
            let error = plan_command_with_now_and_capabilities(
                ast,
                1,
                Timestamp::new(0, 0),
                &windows_like_caps(),
            )
            .unwrap_err();
            assert!(
                error
                    .message
                    .contains("Unix mode bits are not supported on Windows"),
                "{args:?} -> {}",
                error.message
            );
        }
    }

    #[test]
    fn omits_traversal_control_plan_without_prune() {
        let ast = parse_command(&argv(&[".", "-name", "*.rs", "-print"])).unwrap();
        let plan = plan_command(ast, 4).unwrap();

        assert!(plan.traversal_control.is_none());
        assert_eq!(plan.runtime_policy.requested_workers, 4);
        assert_eq!(
            plan.runtime_policy.commit,
            crate::runtime_policy::CommitPolicy::Relaxed
        );
    }

    #[test]
    fn keeps_traversal_control_plan_when_prune_can_change_descent() {
        let ast = parse_command(&argv(&[".", "-name", "cache", "-prune", "-o", "-print"])).unwrap();
        let plan = plan_command(ast, 4).unwrap();

        assert!(plan.traversal_control.is_some());
        assert_eq!(plan.runtime_policy.requested_workers, 4);
    }

    #[test]
    fn depth_mode_derives_subtree_barrier_commit_policy() {
        let ast = parse_command(&argv(&[".", "-delete"])).unwrap();
        let plan = plan_command(ast, 4).unwrap();

        assert_eq!(
            plan.runtime_policy.commit,
            crate::runtime_policy::CommitPolicy::RelaxedWithSubtreeBarriers
        );
        assert!(plan.traversal_control.is_none());
    }
}
