# Priority 0 runtime design

This document defines the remaining Priority 0 work before implementation:

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

`-X` is a global option, not an expression predicate. It affects path-emitting
output actions that produce newline-delimited names (`-print`, `-printx`, and
the path portion of future BSD output aliases). `-print0` remains safe by
construction and is emitted even when `-X` is active. Metadata actions such as
`-ls`, `-printf`, and `-exec` are not filtered in the first implementation
because they do not have the plain `xargs` pathname contract; their existing
semantics remain unchanged.

When a rejected path is encountered, `rfd` should write one diagnostic to
stderr and skip that output record. The traversal continues. The skipped
record is not an action failure and does not change the process exit status.

### Data flow

1. Add `xargs_safe: bool` to `CompatibilityOptions`.
2. Recognize `-X` in `parse_leading_option` before expression/path splitting.
3. Carry the flag through `ExecutionPlan` unchanged.
4. Add `OutputPresentation::xargs_safe` and pass it to the output renderer.
5. Make `render_action_output_with_presentation` return an explicit outcome for
   a filtered record, for example `RenderedAction::Skipped`, rather than
   overloading an absent action with an internal error.
6. The ordered sink, parallel worker sink, and broker path all treat
   `Skipped` as a successful true action with one stderr diagnostic.

The diagnostic should identify the path and the reason, while preserving raw
bytes where the platform allows it. A first stable form is:

`rfd: <path>: skipped by -X because the name contains a character unsafe for xargs`

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
- Skip diagnostics do not set `had_action_failures`.
- Generated implicit `-print` obeys `-X`.

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

