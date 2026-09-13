# Priority 0 runtime design

All four steps below are implemented: `-X` and `-exit` landed first, then the
BSD unit durations and the NetBSD `-since` aliases. The semantics the document
left open are settled in the code as follows.

- **Unit durations compare raw seconds.** FreeBSD's `f_Xtime` takes the
  `F_EXACTTIME` branch for a suffixed value and compares `now - xtime` against
  the duration in seconds, with no day rounding and no `-daystart`; only the
  unsuffixed form rounds up to whole 24-hour periods. rfd matches that, so
  `-mtime 1h30m` matches a file whose whole-second age is exactly 5400 and
  `-mtime +1h` is a strict comparison. The minute primaries keep GNU's rounding
  rather than FreeBSD's ceiling, since `-mmin` is a GNU spelling.
- **The aliases are the `-newerXY` spellings.** `-since` is `-newermt`,
  `-asince` is `-newerat`, `-csince` is `-newerct`.
- **Birth-time predicates read the same timestamp `%B` renders**, and a
  panicking worker now reports an error instead of leaving the run waiting for
  work that will never arrive.

This document defined the remaining Priority 0 work before implementation:

- BSD `-X` safe-for-`xargs` output filtering.
- NetBSD `-exit [status]` with explicit process status propagation.
- BSD time arguments and birth-time predicates beyond the current numeric GNU
  subset.

The design keeps the current byte-preserving path model, ordered/parallel
execution split, capability gates, and planning-time diagnostics.

## `-X`: safe-for-`xargs` output policy

### Semantics

BSD `-X` rejects pathnames containing any delimiter that plain `xargs` treats
as syntactically significant:

- space
- tab
- newline
- single quote
- double quote
- backslash

The check is byte based. It must inspect the displayed path bytes after the
platform path renderer has applied its separator policy, and it must not use
lossy UTF-8 conversion.

`-X` is a global traversal policy, not an output renderer. It rejects an entry
before the expression is evaluated, regardless of whether the expression uses
`-print`, `-print0`, `-exec`, `-false`, or another action. `-printx` is a
separate NetBSD escaping primary; it does not change the `-X` rejection rule.
Unsafe directory entries are skipped as entries but are still descended into;
their descendants are checked independently. This matches macOS `find`, which
reports `illegal path` for each unsafe entry and continues the walk.

When a rejected path is encountered, `rfd` writes one `illegal path` diagnostic
to stderr and skips that entry before evaluating the expression. The traversal
continues, so descendants of an unsafe directory can still be visited. The
diagnostic is a runtime error and therefore gives the command a nonzero exit
status.

### Data flow

1. Add `xargs_safe: bool` to `CompatibilityOptions`.
2. Recognize `-X` in `parse_leading_option` before expression/path splitting.
3. Carry the flag through `ExecutionPlan` unchanged.
4. Add an entry-level `xargs_safe_path` check in the ordered and parallel
   walkers before traversal-control evaluation and expression dispatch.
5. Emit the centralized `illegal path` diagnostic through the existing runtime
   error channels and mark the run as having a runtime error.
6. Continue descent decisions for unsafe directories using an allow control,
   while suppressing evaluation and output for the unsafe entry itself.

The diagnostic should identify the path and the reason, while preserving raw
bytes where the platform allows it. The macOS wording is:

`rfd: <path>: illegal path`

The exact wording should be centralized so ordered and parallel execution
match.

### Interaction with defaults and parallelism

`-X` does not force ordered execution. In relaxed parallel mode, diagnostics
may be interleaved with output according to the existing broker ordering. The
filter is local and stateless, so it is safe to evaluate in workers.

`-X` must not alter implicit-print insertion. A command with only predicates
still gets the normal `-print`; that generated print is subject to `-X`.

### Tests

- Parser recognizes `-X` as a leading option and rejects a missing option
  argument only where applicable; `-X` itself takes none.
- Space, tab, newline, quote, and backslash path bytes are skipped for
  `-print`.
- Safe names are emitted unchanged.
- `-print0` emits unsafe names despite `-X`.
- Raw non-UTF-8 names follow byte semantics.
- Skip diagnostics set the normal runtime-error result, as on macOS/BSD.
- `-print0`, `-exec`, and `-false` still skip unsafe entries because filtering
  happens before expression evaluation.

## NetBSD `-exit [status]`

### Semantics

`-exit` stops traversal immediately after the current expression position has
been evaluated. The optional `status` is a decimal integer. If omitted, the
status is zero. The status must fit the process exit-code range accepted by the
platform; use `0..=255` for the portable CLI contract and reject other values
during parsing.

The optional argument is consumed only when the next token is an unambiguous
decimal status. Tokens beginning with `-`, operators, parentheses, commas, and
ordinary non-decimal expression tokens remain available to the expression
parser. This preserves commands such as:

```text
rfd . -name stop -exit -print
rfd . -name stop -exit 17 -print
```

The first command exits zero; the second exits 17 after evaluating the
preceding `-name` and `-exit` actions for the matching entry.

### AST and planner

Replace the current aliasing of `-exit` to `Action::Quit` with a distinct
variant:

```text
Action::Exit { status: u8 }
```

The parser creates `status: 0` when no numeric argument follows. `-quit`
continues to use its existing successful stop action.

The planner lowers this to `RuntimeAction::Exit { status: u8 }` and marks the
plan as ordered-only. This avoids ambiguous results when multiple parallel
workers are already evaluating entries and makes “stop immediately” match BSD
behavior. The plan may still use multiple internal evaluators in the ordered
pipeline where that is already supported, but output and action commit order
must remain the existing ordered sequence.

### Runtime status model

Extend `RuntimeStatus` with an optional requested exit code:

```text
requested_exit: Option<u8>
```

`RuntimeStatus::stop_requested()` remains the successful `-quit` control. A new
constructor records both stop and the explicit exit code. Status merging keeps
the first explicit exit request and preserves action-failure information for
diagnostics.

Extend `RunSummary` with `requested_exit: Option<u8>`. The CLI chooses the
explicit requested status when present; otherwise it keeps the current rule of
returning 1 for runtime errors or action failures and 0 for success.

The ordered engine stops emitting new entries as soon as the exit status is
observed. The parallel engine is not selected for plans containing `-exit`, so
there is no race between several workers requesting different statuses.

### Tests

- Parser distinguishes `-quit` from `-exit`.
- `-exit` defaults to status 0.
- `-exit 17` consumes the status and preserves the following expression.
- Invalid, negative, overflowing, or non-decimal statuses are planning/parsing
  errors with a stable diagnostic.
- Ordered execution evaluates preceding actions, stops at `-exit`, and returns
  the requested code.
- `-exit 17` inside `-not`, `-or`, and comma/sequence expressions preserves
  existing expression control flow while still stopping the traversal.
- A runtime error before `-exit` is reported, but the explicit exit status is
  returned when the exit action is reached.

## BSD time arguments

### Scope

The current time parser accepts GNU-style decimal and fractional magnitudes.
BSD syntax needs a separate duration parser so existing GNU boundary behavior
does not change accidentally.

The first BSD-compatible phase supports compound units for the time primaries:

- `-Btime`
- `-atime`
- `-ctime`
- `-mtime`

Supported suffixes are:

- `s` seconds
- `m` minutes
- `h` hours
- `d` days
- `w` weeks

An argument may contain multiple components, such as `1h30m` or `2d12h`.
Without a suffix, `-Btime`, `-atime`, `-ctime`, and `-mtime` retain their
existing day-based interpretation. `-Bmin`, `-amin`, `-cmin`, and `-mmin`
remain minute-based primaries and continue to accept the existing comparison
prefixes.

### Representation

Add a `TimeAmount::Compound` representation or a parallel
`BsdDurationAmount` that stores a checked integer number of nanoseconds (or
seconds plus nanoseconds). Parsing must reject empty components, repeated
signs, unknown suffixes, overflow, and fractional components mixed with unit
suffixes unless a later BSD compatibility slice explicitly adds them.

Convert the parsed duration into the existing `TimeComparison` form only after
the comparison sign has been removed. `RelativeTimeMatcher` already handles
less-than, exact, and greater-than windows; it needs a unit-independent path
for arbitrary durations rather than assuming `Minutes` or `Days`.

Birth-time matching continues to use `EntryContext::active_birth_time` and the
`PlatformFeature::BirthTime` capability gate. On a platform without birth time,
`-Btime` and `-Bmin` fail during planning with the existing unsupported
diagnostic.

### BSD reference-time aliases

After compound durations are in place, add NetBSD reference-time aliases as a
separate parser/planner step:

- `-asince DATE`
- `-csince DATE`
- `-since DATE`
- `-newerat DATE`
- `-newerct DATE`
- `-newermt DATE`

These consume one complete argument and use the existing literal-time parser
as the initial accepted date subset (`@seconds`, ISO-like dates, optional
offsets). Full `parsedate(3)` natural-language input should remain a follow-up
compatibility task rather than being guessed from shell tokens.

### Tests

- Compound unit parsing for each supported suffix and combinations.
- Comparison prefixes `+` and `-` with compound durations.
- Exact boundary tests around seconds, minutes, hours, days, and weeks.
- Overflow and malformed-component diagnostics.
- Birth-time capability gating for `-Btime` and `-Bmin`.
- Differential tests against FreeBSD/macOS for `-Btime` and against NetBSD for
  the reference-time aliases once those aliases are implemented.

## Implementation order

1. Implement `-X` because it is local to rendering and does not change the
   execution-control model.
2. Implement `-exit [status]`, including the ordered-only planner policy and
   `RunSummary` exit propagation.
3. Add the compound BSD duration parser and generalize relative-time matching.
4. Add NetBSD reference-time aliases using the existing literal-time subset.

Each step should land in its own commit with focused parser, planner, runtime,
and differential tests. Priority 1 work (`-s`, ACLs, and other BSD metadata)
remains deferred until these Priority 0 semantics are complete.
