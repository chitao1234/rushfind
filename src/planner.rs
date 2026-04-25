use crate::account::{
    PrincipalId, canonicalize_sid_principal, resolve_group_principal, resolve_user_principal,
};
use crate::ast::{
    Action, CommandAst, CompatibilityOptions, Expr, FileTypeMatcher, GlobalOption, Predicate,
};
use crate::diagnostics::Diagnostic;
use crate::exec::{
    BatchedExecAction, ExecBatchId, ExecSemantics, ImmediateExecAction, compile_batched_exec,
    compile_immediate_exec,
};
use crate::file_flags::{FileFlagsMatcher, parse_flags_argument};
use crate::file_output::{FileOutputId, FileOutputTerminator, PlannedFileOutput};
use crate::follow::FollowMode;
use crate::identity::FileIdentity;
use crate::numeric::{NumericComparison, parse_numeric_argument};
use crate::optimizer::optimize_read_only_and_chains;
use crate::pattern::{CompiledGlob, GlobCaseMode, GlobSlashMode};
use crate::perm::{PermMatcher, parse_perm_argument};
use crate::platform::{PlatformCapabilities, PlatformFeature, SupportLevel, active_capabilities};
use crate::printf::{
    PrintfAtom, PrintfDirectiveKind, PrintfProgram, PrintfTimeFamily, compile_printf_program,
};
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

pub use crate::platform::filesystem::ReparseTypeClass;

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
    pub compatibility_options: CompatibilityOptions,
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
    Sequence(Arc<[RuntimeExpr]>),
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

    pub fn sequence(items: Vec<Self>) -> Self {
        Self::Sequence(items.into())
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
    Flags(FileFlagsMatcher),
    ReparseType(ReparseTypeClass),
    Size(SizeMatcher),
    Empty,
    Used(UsedMatcher),
    RelativeTime(RelativeTimeMatcher),
    Newer(NewerMatcher),
    Type(FileTypeMatcher),
    XType(FileTypeMatcher),
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

fn parse_reparse_type(raw: &std::ffi::OsStr) -> Result<ReparseTypeClass, Diagnostic> {
    match raw.to_string_lossy().as_ref() {
        "symbolic" => Ok(ReparseTypeClass::Symbolic),
        "mount-point" => Ok(ReparseTypeClass::MountPoint),
        "app-exec-link" => Ok(ReparseTypeClass::AppExecLink),
        "wsl-symlink" => Ok(ReparseTypeClass::WslSymlink),
        "af-unix" => Ok(ReparseTypeClass::AfUnix),
        "cloud" => Ok(ReparseTypeClass::Cloud),
        "projfs" => Ok(ReparseTypeClass::ProjFs),
        "other" => Ok(ReparseTypeClass::Other),
        other => Err(Diagnostic::new(
            format!("unknown -reparse-type name `{other}`"),
            1,
        )),
    }
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
        start_paths,
        compatibility_options,
        expr,
        ..
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
        compatibility_options,
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
        RuntimeExpr::Sequence(items) => {
            for item in items.iter() {
                populate_action_profile(item, profile);
            }
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
            GlobalOption::Version | GlobalOption::Help => FollowMode::Physical,
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
    flag: &'static str,
    program: &PrintfProgram,
    capabilities: &PlatformCapabilities,
    state: &mut PlanningState,
) -> Result<(), Diagnostic> {
    for atom in &program.atoms {
        let PrintfAtom::Directive(directive) = atom else {
            continue;
        };
        match directive.kind {
            PrintfDirectiveKind::UserSid => {
                if !capabilities.uses_windows_native_output_contract() {
                    return Err(Diagnostic::unsupported("%US is only supported on Windows"));
                }
            }
            PrintfDirectiveKind::GroupSid => {
                if !capabilities.uses_windows_native_output_contract() {
                    return Err(Diagnostic::unsupported("%GS is only supported on Windows"));
                }
            }
            PrintfDirectiveKind::UserId | PrintfDirectiveKind::GroupId => {
                require_platform_feature(capabilities, PlatformFeature::NumericOwnership, state)?;
            }
            PrintfDirectiveKind::ModeOctal | PrintfDirectiveKind::ModeSymbolic => {
                require_platform_feature(capabilities, PlatformFeature::ModeBits, state)?;
            }
            PrintfDirectiveKind::FullTimestamp(PrintfTimeFamily::Birth)
            | PrintfDirectiveKind::TimestampPart {
                family: PrintfTimeFamily::Birth,
                ..
            } => {}
            PrintfDirectiveKind::Device
            | PrintfDirectiveKind::Blocks512
            | PrintfDirectiveKind::Blocks1024 => {
                if capabilities.uses_windows_native_output_contract() {
                    return Err(Diagnostic::new(
                        format!("unsupported {flag} directive on Windows"),
                        1,
                    ));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn windows_ownership_predicates_supported(capabilities: &PlatformCapabilities) -> bool {
    matches!(
        capabilities.support(PlatformFeature::WindowsOwnershipPredicates),
        SupportLevel::Exact
    )
}

fn unsupported_windows_numeric_ownership(
    flag: &'static str,
    capabilities: &PlatformCapabilities,
) -> Option<Diagnostic> {
    if !matches!(
        capabilities.support(PlatformFeature::NumericOwnership),
        SupportLevel::Unsupported(_)
    ) {
        return None;
    }

    if !windows_ownership_predicates_supported(capabilities) {
        return None;
    }

    match flag {
        "-uid" => Some(Diagnostic::unsupported(
            "-uid is not supported on Windows; use -owner-sid for SID matching",
        )),
        "-gid" => Some(Diagnostic::unsupported(
            "-gid is not supported on Windows; use -group-sid for SID matching",
        )),
        _ => None,
    }
}

fn require_windows_ownership_predicate(
    flag: &'static str,
    capabilities: &PlatformCapabilities,
) -> Result<(), Diagnostic> {
    if windows_ownership_predicates_supported(capabilities) {
        return Ok(());
    }

    Err(Diagnostic::unsupported(format!(
        "{flag} is only supported on Windows"
    )))
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
        Expr::Sequence(items) => {
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
            Ok(RuntimeExpr::sequence(lowered))
        }
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
        predicate @ (Predicate::MaxDepth(_)
        | Predicate::MinDepth(_)
        | Predicate::Depth
        | Predicate::Prune
        | Predicate::XDev) => lower_traversal_predicate(predicate, traversal, state, capabilities),
        predicate @ (Predicate::Readable | Predicate::Writable | Predicate::Executable) => {
            lower_access_predicate(predicate, state, capabilities)
        }
        predicate @ (Predicate::Name { .. }
        | Predicate::Path { .. }
        | Predicate::Regex { .. }
        | Predicate::RegexType(_)
        | Predicate::FsType(_)
        | Predicate::LName { .. }) => {
            lower_pattern_predicate(predicate, runtime, state, capabilities)
        }
        predicate @ (Predicate::Inum(_) | Predicate::Links(_) | Predicate::SameFile(_)) => {
            lower_identity_predicate(predicate, follow_mode)
        }
        predicate @ (Predicate::Uid(_)
        | Predicate::Gid(_)
        | Predicate::User(_)
        | Predicate::Group(_)
        | Predicate::Owner(_)
        | Predicate::OwnerSid(_)
        | Predicate::GroupSid(_)
        | Predicate::NoUser
        | Predicate::NoGroup) => lower_ownership_predicate(predicate, state, capabilities),
        predicate @ (Predicate::Perm(_)
        | Predicate::Flags(_)
        | Predicate::ReparseType(_)
        | Predicate::Size(_)
        | Predicate::Empty) => lower_metadata_predicate(predicate, state, capabilities),
        predicate @ (Predicate::Used(_)
        | Predicate::ATime(_)
        | Predicate::CTime(_)
        | Predicate::MTime(_)
        | Predicate::AMin(_)
        | Predicate::CMin(_)
        | Predicate::MMin(_)
        | Predicate::DayStart) => lower_temporal_predicate(predicate, state),
        predicate @ (Predicate::Newer(_)
        | Predicate::ANewer(_)
        | Predicate::CNewer(_)
        | Predicate::NewerXY { .. }) => {
            lower_newer_predicate(predicate, follow_mode, state, capabilities)
        }
        Predicate::Type(kind) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Type(kind))),
        Predicate::XType(kind) => Ok(RuntimeExpr::Predicate(RuntimePredicate::XType(kind))),
        Predicate::Compatibility(_) => Ok(RuntimeExpr::Barrier),
        Predicate::True => Ok(RuntimeExpr::Predicate(RuntimePredicate::True)),
        Predicate::False => Ok(RuntimeExpr::Predicate(RuntimePredicate::False)),
    }
}

fn lower_traversal_predicate(
    predicate: Predicate,
    traversal: &mut TraversalOptions,
    state: &mut PlanningState,
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
        _ => unreachable!("predicate dispatch guarantees traversal predicate"),
    }
}

fn lower_access_predicate(
    predicate: Predicate,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    require_platform_feature(capabilities, PlatformFeature::AccessPredicates, state)?;
    Ok(RuntimeExpr::Predicate(match predicate {
        Predicate::Readable => RuntimePredicate::Readable,
        Predicate::Writable => RuntimePredicate::Writable,
        Predicate::Executable => RuntimePredicate::Executable,
        _ => unreachable!("predicate dispatch guarantees access predicate"),
    }))
}

fn lower_pattern_predicate(
    predicate: Predicate,
    runtime: &mut RuntimeRequirements,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    match predicate {
        Predicate::Name {
            pattern,
            case_insensitive,
        } => Ok(RuntimeExpr::Predicate(RuntimePredicate::Name(
            compile_case_glob(
                if case_insensitive { "-iname" } else { "-name" },
                pattern.as_os_str(),
                case_insensitive,
                state,
                capabilities,
            )?,
        ))),
        Predicate::Path {
            pattern,
            case_insensitive,
        } => Ok(RuntimeExpr::Predicate(RuntimePredicate::Path(
            compile_normalized_glob(
                if case_insensitive { "-ipath" } else { "-path" },
                pattern.as_os_str(),
                case_insensitive,
                state,
                capabilities,
            )?,
        ))),
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
        Predicate::LName {
            pattern,
            case_insensitive,
        } => Ok(RuntimeExpr::Predicate(RuntimePredicate::LName(
            compile_normalized_glob(
                if case_insensitive {
                    "-ilname"
                } else {
                    "-lname"
                },
                pattern.as_os_str(),
                case_insensitive,
                state,
                capabilities,
            )?,
        ))),
        _ => unreachable!("predicate dispatch guarantees pattern predicate"),
    }
}

fn compile_case_glob(
    flag: &'static str,
    pattern: &std::ffi::OsStr,
    case_insensitive: bool,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<CompiledGlob, Diagnostic> {
    if case_insensitive {
        require_platform_feature(capabilities, PlatformFeature::CaseInsensitiveGlob, state)?;
    }
    compile_glob(flag, pattern, case_insensitive, GlobSlashMode::Literal)
}

fn compile_normalized_glob(
    flag: &'static str,
    pattern: &std::ffi::OsStr,
    case_insensitive: bool,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<CompiledGlob, Diagnostic> {
    let normalized_pattern = normalize_match_pattern(pattern);
    compile_case_glob(
        flag,
        normalized_pattern.as_ref(),
        case_insensitive,
        state,
        capabilities,
    )
}

fn lower_identity_predicate(
    predicate: Predicate,
    follow_mode: FollowMode,
) -> Result<RuntimeExpr, Diagnostic> {
    Ok(RuntimeExpr::Predicate(match predicate {
        Predicate::Inum(raw) => {
            RuntimePredicate::Inum(parse_numeric_argument("-inum", raw.as_os_str())?)
        }
        Predicate::Links(raw) => {
            RuntimePredicate::Links(parse_numeric_argument("-links", raw.as_os_str())?)
        }
        Predicate::SameFile(path) => {
            RuntimePredicate::SameFile(resolve_samefile_reference(&path, follow_mode)?)
        }
        _ => unreachable!("predicate dispatch guarantees identity predicate"),
    }))
}

fn lower_ownership_predicate(
    predicate: Predicate,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    match predicate {
        Predicate::Uid(raw) => lower_numeric_owner("-uid", raw, state, capabilities),
        Predicate::Gid(raw) => lower_numeric_owner("-gid", raw, state, capabilities),
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
        Predicate::Owner(raw) => {
            require_windows_ownership_predicate("-owner", capabilities)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::User(
                resolve_user_principal(raw.as_os_str())?,
            )))
        }
        Predicate::OwnerSid(raw) => {
            require_windows_ownership_predicate("-owner-sid", capabilities)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::User(
                canonicalize_sid_principal(raw.as_os_str())?,
            )))
        }
        Predicate::GroupSid(raw) => {
            require_windows_ownership_predicate("-group-sid", capabilities)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Group(
                canonicalize_sid_principal(raw.as_os_str())?,
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
        _ => unreachable!("predicate dispatch guarantees ownership predicate"),
    }
}

fn lower_numeric_owner(
    flag: &'static str,
    raw: std::ffi::OsString,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    if let Some(error) = unsupported_windows_numeric_ownership(flag, capabilities) {
        return Err(error);
    }
    require_platform_feature(capabilities, PlatformFeature::NumericOwnership, state)?;
    let comparison = parse_numeric_argument(flag, raw.as_os_str())?;
    Ok(RuntimeExpr::Predicate(match flag {
        "-uid" => RuntimePredicate::Uid(comparison),
        "-gid" => RuntimePredicate::Gid(comparison),
        _ => unreachable!("only uid and gid numeric ownership flags are supported"),
    }))
}

fn lower_metadata_predicate(
    predicate: Predicate,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    match predicate {
        Predicate::Perm(raw) => {
            require_platform_feature(capabilities, PlatformFeature::ModeBits, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Perm(
                parse_perm_argument(raw.as_os_str())?,
            )))
        }
        Predicate::Flags(raw) => {
            require_platform_feature(capabilities, PlatformFeature::FileFlags, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::Flags(
                parse_flags_argument(raw.as_os_str(), crate::platform::active_flag_specs())?,
            )))
        }
        Predicate::ReparseType(raw) => {
            require_platform_feature(capabilities, PlatformFeature::ReparseType, state)?;
            Ok(RuntimeExpr::Predicate(RuntimePredicate::ReparseType(
                parse_reparse_type(raw.as_os_str())?,
            )))
        }
        Predicate::Size(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Size(
            parse_size_argument(raw.as_os_str())?,
        ))),
        Predicate::Empty => Ok(RuntimeExpr::Predicate(RuntimePredicate::Empty)),
        _ => unreachable!("predicate dispatch guarantees metadata predicate"),
    }
}

fn lower_temporal_predicate(
    predicate: Predicate,
    state: &mut PlanningState,
) -> Result<RuntimeExpr, Diagnostic> {
    match predicate {
        Predicate::Used(raw) => Ok(RuntimeExpr::Predicate(RuntimePredicate::Used(
            UsedMatcher {
                comparison: parse_time_comparison("-used", raw.as_os_str())?,
            },
        ))),
        Predicate::ATime(raw) => lower_relative_time("-atime", raw, TimestampKind::Access, state),
        Predicate::CTime(raw) => lower_relative_time("-ctime", raw, TimestampKind::Change, state),
        Predicate::MTime(raw) => {
            lower_relative_time("-mtime", raw, TimestampKind::Modification, state)
        }
        Predicate::AMin(raw) => lower_relative_minutes("-amin", raw, TimestampKind::Access, state),
        Predicate::CMin(raw) => lower_relative_minutes("-cmin", raw, TimestampKind::Change, state),
        Predicate::MMin(raw) => {
            lower_relative_minutes("-mmin", raw, TimestampKind::Modification, state)
        }
        Predicate::DayStart => {
            state.temporal.daystart_active = true;
            Ok(RuntimeExpr::Barrier)
        }
        _ => unreachable!("predicate dispatch guarantees temporal predicate"),
    }
}

fn lower_relative_time(
    flag: &'static str,
    raw: std::ffi::OsString,
    timestamp_kind: TimestampKind,
    state: &PlanningState,
) -> Result<RuntimeExpr, Diagnostic> {
    lower_relative_time_with_unit(flag, raw, timestamp_kind, RelativeTimeUnit::Days, state)
}

fn lower_relative_minutes(
    flag: &'static str,
    raw: std::ffi::OsString,
    timestamp_kind: TimestampKind,
    state: &PlanningState,
) -> Result<RuntimeExpr, Diagnostic> {
    lower_relative_time_with_unit(flag, raw, timestamp_kind, RelativeTimeUnit::Minutes, state)
}

fn lower_relative_time_with_unit(
    flag: &'static str,
    raw: std::ffi::OsString,
    timestamp_kind: TimestampKind,
    unit: RelativeTimeUnit,
    state: &PlanningState,
) -> Result<RuntimeExpr, Diagnostic> {
    Ok(RuntimeExpr::Predicate(RuntimePredicate::RelativeTime(
        parse_relative_time_argument(
            flag,
            raw.as_os_str(),
            timestamp_kind,
            unit,
            state.temporal.relative_baseline()?,
            state.temporal.daystart_active,
        )?,
    )))
}

fn lower_newer_predicate(
    predicate: Predicate,
    follow_mode: FollowMode,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    let matcher = match predicate {
        Predicate::Newer(path) => {
            resolve_reference_matcher("-newer", 'm', 'm', path.as_os_str(), follow_mode)?
        }
        Predicate::ANewer(path) => {
            resolve_reference_matcher("-anewer", 'a', 'm', path.as_os_str(), follow_mode)?
        }
        Predicate::CNewer(path) => {
            resolve_reference_matcher("-cnewer", 'c', 'm', path.as_os_str(), follow_mode)?
        }
        Predicate::NewerXY {
            current,
            reference,
            reference_arg,
        } => {
            if current == 'B' || reference == 'B' {
                require_platform_feature(capabilities, PlatformFeature::BirthTime, state)?;
            }
            resolve_reference_matcher(
                "-newerXY",
                current,
                reference,
                reference_arg.as_os_str(),
                follow_mode,
            )?
        }
        _ => unreachable!("predicate dispatch guarantees newer predicate"),
    };
    Ok(RuntimeExpr::Predicate(RuntimePredicate::Newer(matcher)))
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
        action @ (Action::Print | Action::Print0 | Action::Quit | Action::Delete) => {
            lower_simple_action(action, state)
        }
        action @ (Action::Printf { .. } | Action::FPrintf { .. }) => {
            lower_printf_action(action, runtime, state, capabilities)
        }
        action @ (Action::FPrint { .. }
        | Action::FPrint0 { .. }
        | Action::Ls
        | Action::Fls { .. }) => lower_output_action(action, state, capabilities),
        action @ (Action::Exec { .. } | Action::ExecDir { .. }) => {
            lower_exec_action(action, runtime, state)
        }
        action @ (Action::Ok { .. } | Action::OkDir { .. }) => {
            lower_prompt_action(action, runtime, state, capabilities)
        }
    }
}

fn lower_simple_action(
    action: Action,
    state: &mut PlanningState,
) -> Result<RuntimeExpr, Diagnostic> {
    Ok(RuntimeExpr::Action(match action {
        Action::Print => RuntimeAction::Output(OutputAction::Print),
        Action::Print0 => RuntimeAction::Output(OutputAction::Print0),
        Action::Quit => RuntimeAction::Quit,
        Action::Delete => {
            state.saw_delete = true;
            RuntimeAction::Delete
        }
        _ => unreachable!("action dispatch guarantees simple action"),
    }))
}

fn lower_printf_action(
    action: Action,
    runtime: &mut RuntimeRequirements,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    match action {
        Action::Printf { format } => {
            let program = compile_action_printf("-printf", format, runtime, state, capabilities)?;
            Ok(RuntimeExpr::Action(RuntimeAction::Printf(program)))
        }
        Action::FPrintf { path, format } => {
            let program = compile_action_printf("-fprintf", format, runtime, state, capabilities)?;
            Ok(RuntimeExpr::Action(RuntimeAction::FilePrintf {
                destination: register_file_output(state, path),
                program,
            }))
        }
        _ => unreachable!("action dispatch guarantees printf action"),
    }
}

fn compile_action_printf(
    flag: &'static str,
    format: OsString,
    runtime: &mut RuntimeRequirements,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<PrintfProgram, Diagnostic> {
    let compiled = compile_printf_program(flag, format.as_os_str())?;
    if compiled.program.requires_mount_snapshot() {
        require_platform_feature(capabilities, PlatformFeature::FsType, state)?;
        runtime.mount_snapshot = true;
    }
    validate_platform_printf_program(flag, &compiled.program, capabilities, state)?;
    state.startup_warnings.extend(compiled.warnings);
    Ok(compiled.program)
}

fn lower_output_action(
    action: Action,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    match action {
        Action::FPrint { path } => Ok(RuntimeExpr::Action(RuntimeAction::FilePrint {
            destination: register_file_output(state, path),
            terminator: FileOutputTerminator::Newline,
        })),
        Action::FPrint0 { path } => Ok(RuntimeExpr::Action(RuntimeAction::FilePrint {
            destination: register_file_output(state, path),
            terminator: FileOutputTerminator::Nul,
        })),
        Action::Ls => {
            require_ls_mode_bits_if_needed(capabilities, state)?;
            Ok(RuntimeExpr::Action(RuntimeAction::Ls))
        }
        Action::Fls { path } => {
            require_ls_mode_bits_if_needed(capabilities, state)?;
            Ok(RuntimeExpr::Action(RuntimeAction::FileLs {
                destination: register_file_output(state, path),
            }))
        }
        _ => unreachable!("action dispatch guarantees output action"),
    }
}

fn require_ls_mode_bits_if_needed(
    capabilities: &PlatformCapabilities,
    state: &mut PlanningState,
) -> Result<(), Diagnostic> {
    if !capabilities.uses_windows_native_output_contract() {
        require_platform_feature(capabilities, PlatformFeature::ModeBits, state)?;
    }
    Ok(())
}

fn lower_exec_action(
    action: Action,
    runtime: &mut RuntimeRequirements,
    state: &mut PlanningState,
) -> Result<RuntimeExpr, Diagnostic> {
    match action {
        Action::Exec { argv, batch } => {
            lower_exec_with_semantics(argv, batch, ExecSemantics::Normal, false, runtime, state)
        }
        Action::ExecDir { argv, batch } => {
            lower_exec_with_semantics(argv, batch, ExecSemantics::DirLocal, true, runtime, state)
        }
        _ => unreachable!("action dispatch guarantees exec action"),
    }
}

fn lower_exec_with_semantics(
    argv: Vec<OsString>,
    batch: bool,
    semantics: ExecSemantics,
    requires_safe_path: bool,
    runtime: &mut RuntimeRequirements,
    state: &mut PlanningState,
) -> Result<RuntimeExpr, Diagnostic> {
    if requires_safe_path {
        runtime.execdir_requires_safe_path = true;
    }

    let action = if batch {
        let id = state.next_exec_batch_id;
        state.next_exec_batch_id += 1;
        RuntimeAction::ExecBatched(compile_batched_exec(id, semantics, &argv)?)
    } else {
        RuntimeAction::ExecImmediate(compile_immediate_exec(semantics, &argv))
    };
    Ok(RuntimeExpr::Action(action))
}

fn lower_prompt_action(
    action: Action,
    runtime: &mut RuntimeRequirements,
    state: &mut PlanningState,
    capabilities: &PlatformCapabilities,
) -> Result<RuntimeExpr, Diagnostic> {
    require_platform_feature(capabilities, PlatformFeature::MessagesLocale, state)?;
    runtime.messages_locale_required = true;

    match action {
        Action::Ok { argv, batch: false } => Ok(RuntimeExpr::Action(RuntimeAction::ExecPrompt(
            compile_immediate_exec(ExecSemantics::Normal, &argv),
        ))),
        Action::Ok { batch: true, .. } => {
            Err(Diagnostic::parse("`-ok` only supports the `;` terminator"))
        }
        Action::OkDir { argv, batch: false } => {
            runtime.execdir_requires_safe_path = true;
            Ok(RuntimeExpr::Action(RuntimeAction::ExecPrompt(
                compile_immediate_exec(ExecSemantics::DirLocal, &argv),
            )))
        }
        Action::OkDir { batch: true, .. } => Err(Diagnostic::parse(
            "`-okdir` only supports the `;` terminator",
        )),
        _ => unreachable!("action dispatch guarantees prompt action"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use crate::account::PrincipalId;
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
            .with(
                PlatformFeature::WindowsOwnershipPredicates,
                SupportLevel::Unsupported(
                    "Windows ownership predicates are only supported on Windows",
                ),
            )
            .with(PlatformFeature::AccessPredicates, SupportLevel::Exact)
            .with(PlatformFeature::MessagesLocale, SupportLevel::Exact)
            .with(PlatformFeature::CaseInsensitiveGlob, SupportLevel::Exact)
            .with(PlatformFeature::ModeBits, SupportLevel::Exact)
    }

    fn windows_like_caps() -> PlatformCapabilities {
        PlatformCapabilities::for_tests()
            .with_windows_native_output_contract()
            .with(PlatformFeature::FsType, SupportLevel::Exact)
            .with(PlatformFeature::SameFileSystem, SupportLevel::Exact)
            .with(PlatformFeature::BirthTime, SupportLevel::Exact)
            .with(PlatformFeature::NamedOwnership, SupportLevel::Exact)
            .with(
                PlatformFeature::NumericOwnership,
                SupportLevel::Unsupported("numeric ownership is not supported on Windows"),
            )
            .with(
                PlatformFeature::WindowsOwnershipPredicates,
                SupportLevel::Exact,
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

    fn generic_unix_caps() -> PlatformCapabilities {
        PlatformCapabilities::for_tests()
            .with(
                PlatformFeature::FsType,
                SupportLevel::Unsupported("`-fstype` is not supported on this platform"),
            )
            .with(PlatformFeature::SameFileSystem, SupportLevel::Exact)
            .with(
                PlatformFeature::BirthTime,
                SupportLevel::Unsupported("birth time is not supported on this platform"),
            )
            .with(
                PlatformFeature::FileFlags,
                SupportLevel::Unsupported("`-flags` is not supported on this platform"),
            )
            .with(PlatformFeature::NamedOwnership, SupportLevel::Exact)
            .with(PlatformFeature::NumericOwnership, SupportLevel::Exact)
            .with(PlatformFeature::AccessPredicates, SupportLevel::Exact)
            .with(
                PlatformFeature::MessagesLocale,
                SupportLevel::Approximate(
                    "interactive locale behavior is approximate on this platform",
                ),
            )
            .with(
                PlatformFeature::CaseInsensitiveGlob,
                SupportLevel::Approximate(
                    "case-insensitive glob matching may differ outside the C locale on this platform",
                ),
            )
            .with(PlatformFeature::ModeBits, SupportLevel::Exact)
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
    fn generic_unix_keeps_xdev_exact() {
        let plan = plan_command_with_now_and_capabilities(
            parse_command(&argv(&[".", "-xdev", "-name", "*.rs"])).unwrap(),
            1,
            Timestamp::new(0, 0),
            &generic_unix_caps(),
        )
        .unwrap();

        assert!(plan.traversal.same_file_system);
    }

    #[test]
    fn generic_unix_warns_for_iname_and_ok() {
        let plan = plan_command_with_now_and_capabilities(
            parse_command(&argv(&[
                ".", "-iname", "*.rs", "-ok", "printf", "%s\\n", "{}", ";",
            ]))
            .unwrap(),
            1,
            Timestamp::new(0, 0),
            &generic_unix_caps(),
        )
        .unwrap();

        assert!(plan.runtime.messages_locale_required);
        assert!(
            plan.startup_warnings
                .iter()
                .any(|warning| { warning.contains("case-insensitive glob matching may differ") })
        );
        assert!(
            plan.startup_warnings
                .iter()
                .any(|warning| { warning.contains("interactive locale behavior is approximate") })
        );
    }

    #[test]
    fn generic_unix_rejects_fstype_flags_and_birth_time() {
        for args in [
            argv(&[".", "-fstype", "ufs"]),
            argv(&[".", "-flags", "nodump"]),
            argv(&[".", "-newerBt", "2024-01-01"]),
            argv(&[".", "-printf", "%F\\n"]),
        ] {
            let error = plan_command_with_now_and_capabilities(
                parse_command(&args).unwrap(),
                1,
                Timestamp::new(0, 0),
                &generic_unix_caps(),
            )
            .unwrap_err();

            assert!(
                error.message.contains("not supported"),
                "{args:?} -> {}",
                error.message
            );
        }
    }

    #[test]
    fn generic_unix_allows_birth_time_printf_directives_to_render_empty() {
        let plan = plan_command_with_now_and_capabilities(
            parse_command(&argv(&[".", "-printf", "%B@\\n"])).unwrap(),
            1,
            Timestamp::new(0, 0),
            &generic_unix_caps(),
        )
        .unwrap();

        assert!(matches!(
            plan.expr,
            RuntimeExpr::Action(RuntimeAction::Printf(_))
        ));
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
            (
                argv(&[".", "-printf", "%D\\n"]),
                "unsupported -printf directive on Windows",
            ),
            (
                argv(&[".", "-printf", "%b\\n"]),
                "unsupported -printf directive on Windows",
            ),
            (
                argv(&[".", "-printf", "%k\\n"]),
                "unsupported -printf directive on Windows",
            ),
            (
                argv(&[".", "-fprintf", "out.txt", "%D\\n"]),
                "unsupported -fprintf directive on Windows",
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
    fn windows_uid_and_gid_diagnostics_suggest_sid_predicates() {
        for (args, needle) in [
            (
                argv(&[".", "-uid", "0"]),
                "-uid is not supported on Windows; use -owner-sid for SID matching",
            ),
            (
                argv(&[".", "-gid", "0"]),
                "-gid is not supported on Windows; use -group-sid for SID matching",
            ),
        ] {
            let error = plan_command_with_now_and_capabilities(
                parse_command(&args).unwrap(),
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
    fn non_windows_rejects_windows_owner_predicates() {
        for (args, needle) in [
            (
                argv(&[".", "-owner", "alice"]),
                "-owner is only supported on Windows",
            ),
            (
                argv(&[".", "-owner-sid", "S-1-5-18"]),
                "-owner-sid is only supported on Windows",
            ),
            (
                argv(&[".", "-group-sid", "S-1-5-32-544"]),
                "-group-sid is only supported on Windows",
            ),
        ] {
            let error = plan_command_with_now_and_capabilities(
                parse_command(&args).unwrap(),
                1,
                Timestamp::new(0, 0),
                &linux_like_caps(),
            )
            .unwrap_err();
            assert!(
                error.message.contains(needle),
                "{args:?} -> {}",
                error.message
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_owner_sid_predicates_lower_to_runtime_principals() {
        let plan = plan_command_with_now_and_capabilities(
            parse_command(&argv(&[
                ".",
                "-owner-sid",
                "S-1-5-18",
                "-group-sid",
                "S-1-5-32-544",
            ]))
            .unwrap(),
            1,
            Timestamp::new(0, 0),
            &windows_like_caps(),
        )
        .unwrap();

        let predicates = predicate_items(&plan.expr);
        assert!(predicates.iter().any(|predicate| matches!(
            predicate,
            RuntimePredicate::User(PrincipalId::Sid(value)) if value == "S-1-5-18"
        )));
        assert!(predicates.iter().any(|predicate| matches!(
            predicate,
            RuntimePredicate::Group(PrincipalId::Sid(value)) if value == "S-1-5-32-544"
        )));
    }

    #[test]
    fn windows_accepts_ls_actions_with_the_native_renderer() {
        for (args, expected_file_outputs) in [
            (argv(&[".", "-ls"]), 0usize),
            (argv(&[".", "-fls", "out.txt"]), 1usize),
        ] {
            let ast = parse_command(&args).unwrap();
            let plan = plan_command_with_now_and_capabilities(
                ast,
                1,
                Timestamp::new(0, 0),
                &windows_like_caps(),
            )
            .unwrap();

            assert_eq!(plan.file_outputs.len(), expected_file_outputs, "{args:?}");
        }
    }

    #[test]
    fn ls_still_requires_mode_bits_without_the_windows_output_contract() {
        let ast = parse_command(&argv(&[".", "-ls"])).unwrap();
        let error = plan_command_with_now_and_capabilities(
            ast,
            1,
            Timestamp::new(0, 0),
            &PlatformCapabilities::for_tests().with(
                PlatformFeature::ModeBits,
                SupportLevel::Unsupported("mode bits are unavailable here"),
            ),
        )
        .unwrap_err();

        assert!(error.message.contains("mode bits are unavailable here"));
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
    fn sequence_with_prune_keeps_traversal_control_plan() {
        let ast = parse_command(&argv(&[".", "-name", "vendor", "-prune", ",", "-false"])).unwrap();
        let plan = plan_command(ast, 4).unwrap();

        assert!(plan.traversal_control.is_some());
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

    #[cfg(windows)]
    fn predicate_items(expr: &RuntimeExpr) -> Vec<&RuntimePredicate> {
        match expr {
            RuntimeExpr::And(items) | RuntimeExpr::Sequence(items) => {
                items.iter().flat_map(predicate_items).collect()
            }
            RuntimeExpr::Predicate(predicate) => vec![predicate],
            RuntimeExpr::Or(left, right) => {
                let mut items = predicate_items(left);
                items.extend(predicate_items(right));
                items
            }
            RuntimeExpr::Not(inner) => predicate_items(inner),
            RuntimeExpr::Action(_) | RuntimeExpr::Barrier => Vec::new(),
        }
    }
}
