# Find compatibility and performance backlog

This is a working backlog for extending `rfd` beyond its current GNU-focused
surface. It covers the BSD `find(1)` family (FreeBSD, OpenBSD, NetBSD, and the
closely related macOS behavior) and implementation issues that are visible in
the traversal and evaluation pipeline.

## Current baseline

`cargo test --all-targets` passes on the current checkout. The parser, planner,
evaluator, ordered walker, relaxed parallel walker, action pipeline, locale
handling, and Unix-family backends are covered by unit, CLI, and differential
tests against GNU findutils 4.11.

BSD spellings that are implemented: `-E`, `-x`, `-d`, `-f PATH`, `-h` (OpenBSD
command-line symlinks), `-X`, `-printx`, `-rm`, `-exit [STATUS]`.

## Landed: performance

- **Traversal decisions no longer read metadata.** `EntryContext`'s
  active-kind check runs before the traversal-link check, so an entry whose
  directory-entry type hint proves it is not a directory costs no `lstat`.
  Measured on 120k files: `-print` 305ms -> 68ms, `-type d` 270ms -> 39ms.
- **BSD metadata reads are one syscall.** `PlatformMetadataView` is built from
  the `Metadata` the caller already holds; flags, birth time and device come
  from that `stat` instead of three more. Measured with a DYLD interposer on
  120k files: 480,484 -> 120,121 `lstat` calls for `-size +0c`, which now runs
  in 0.12s against GNU find's 0.15s. Linux and the generic Unix tier are
  unchanged (their flags need `FS_IOC_GETFLAGS` and their birth time and mount
  id need `statx`).
- **The ordered engine streams.** It previously walked the whole tree before
  releasing a single result: 788ms to first output, 148MB RSS, and stderr
  diagnostics ahead of all stdout. It is now a bounded, sequence-numbered
  pipeline with a collector that owns the sink; see
  [`ordered-pipeline-design.md`](ordered-pipeline-design.md). On 120k files the
  single-worker path went 855ms/148MB -> 136ms/3.2MB and now matches GNU find's
  traversal order exactly.
- **Idle workers wake on enqueue.** The scheduler's 10ms timed wait (which
  could also lose a wake-up) is replaced by a generation counter.
- **Per-entry bookkeeping copies are gone.** `ancestor_barriers` was cloned
  into every child and never read; `ancestry` was rebuilt per directory
  descent; `EntryTicket`/`ScheduledEntry` carried both. Post-order chunk
  publishing no longer rebuilds a `PendingPath` per child either.
- **Printed records cost one allocation** instead of building an
  exactly-sized path vector and then reallocating it for the terminator.
- **Glob matching is linear.** Both matchers now scan greedily with a single
  backtrack point instead of recursing over every split, so the patterns that
  used to be unbounded now match GNU find's time: `*a*a*a*a*a*a*b` went from
  11994ms to 5.8ms and eight or more repetitions from over 20s to 5.5ms. The
  byte and encoded matchers were rewritten together, and on 120k files the
  matcher no longer shows up at all (`-name 'f*0*0*0*9'` costs the same as
  `-name 'f000*'`).

## Landed: GNU compatibility

- **Looping entries are skipped, not evaluated.** A symlink loop made both
  walkers evaluate the entry before the diagnostic; GNU reports it and
  evaluates nothing for that path.
- **Path components follow gnulib.** `%f`, `%h`, `-name`/`-iname` and
  `-execdir`'s `{}` no longer inherit Rust's `Path::file_name()`/`parent()`
  behaviour, which drops `.`, `..`, `/` and trailing separators. A 32-spelling
  start-path matrix went from 152 divergences from GNU find to zero.
- **`-ls`/`-fls` escape names** with GNU's byte-wise rule, in the name and in
  the symlink target, independently of the locale and of a terminal.
- **`-printf` time selectors are complete.** `%TC`, `%Te`, `%Tk`, `%Tl`,
  `%Tn`, `%Ts` and `%v` are implemented; every other selector is handed to the
  host `strftime`, which is what GNU find does, so unknown selectors render
  identically on the same host.
- **`-printf` field flags match**: the space flag is accepted, and the zero
  flag pads string fields the way the host C library does.
- **Unrecognized `-printf` directives warn and continue** (once per occurrence
  in the format) instead of failing the run, matching GNU find's exit status.
- **`-ignore_readdir_race` works.** Diagnostics carry the OS error behind them
  and the walkers drop ENOENT/ESTALE ones for entries discovered through a
  listing, while permission and I/O failures and command-line roots are always
  reported.
- **`-O0` means what GNU means**: the written test order is kept instead of
  reordering the cheap predicates.
- **Dead design surface removed**: `ExecutionPlan::parallel_policy`,
  `optimizer::Requirement`, `OutputPresentation`, `EntryTicket`.

## Deferred, with rationale

### Encoded matching still allocates for non-ASCII candidates

An ASCII pattern against an ASCII candidate goes through the byte matcher, so a
UTF-8 locale costs what the C locale costs for them (120k names, `-ipath`:
73.0ms against 72.6ms). A candidate with any non-ASCII byte still materializes
its text units, plus the boxed iterator `decode_units` returns, on every
evaluation - about 0.4us per name. On 4000 names of 33 CJK characters,
`-ipath '*/日本語*'` takes 8.2ms where GNU find takes 4.4ms.

Removing those two allocations means indexing the matcher by byte offset, which
rewrites the scan core the differential coverage was built against. The
remaining cost is small and confined to non-ASCII names.

### `-mtime`-family second boundary

GNU find 4.11 uses `timespec_cmp` for the window comparison but adjusts the
origin by `DAYSECS - 1` in `parse_time`, so a file whose age is just under
`N days + 1s` satisfies `-mtime -N` *and* `-mtime N` *and* `-mtime +(N-1)`.
Measured: a file aged 86400.96s matches all three. `rfd`'s windows are
disjoint.

This is upstream's behaviour on master as well (the April 2026 change removed
`difftime` usage, not the day adjustment), so the project has to decide whether
to reproduce a window that overlaps by up to a second or to keep the disjoint
windows and document the difference. `-mmin` and friends have no such offset
and already agree with GNU exactly.

### Per-directory child buffering

`read_children` materializes one directory before descending. GNU find's `fts`
does the same, and measured RSS on a 200k-entry directory is 33MB for GNU find
against 40MB for `rfd`, so this is not a divergence worth a rewrite. Streaming
through an iterator frame remains possible for the ordered walker if a workload
ever shows it matters.

### Output batching

Each rendered record is still its own broker message. Merging consecutive
records into byte-bounded batches would cut channel traffic, but it changes
flush and atomicity boundaries, so it should follow the measurement the
original backlog asked for (queue wait time, bytes per message, file-output
lock contention) rather than precede it.

## Remaining compatibility work

### BSD traversal and metadata

- `-s` sorted traversal (FreeBSD/NetBSD/macOS). Needs a per-directory sort and
  an ordered-only execution policy, since a global sort of the output is not
  equivalent.
- `-acl` (FreeBSD). Needs an ACL reader behind a capability gate; planning must
  fail explicitly on platforms without one rather than infer ACL presence from
  mode bits.
- BSD time arguments: compound durations (`-mtime 1h30m`) and the NetBSD
  reference-time aliases (`-asince`, `-csince`, `-since`, `-newerat`,
  `-newerct`, `-newermt`). The literal-time parser is the intended backend; see
  `priority0-runtime-design.md` for the accepted subset.
- `-flags` on macOS knows `arch`, `nodump` and `uchg`; the host also defines
  `hidden`, `opaque` and others that `-flags +hidden` should accept.

### Literal time parsing

Accepted today: `@SECONDS[.FRACTION]`, `YYYY-MM-DD`, `YYYYMMDD`, and
`[T| ]HH:MM[:SS][.FRACTION][Z|±HH[:MM]]`. Missing: natural-language input
(`now`, `yesterday`, `1 day ago`, `Jun 15 2024`), a bare number, compact date
times (`202406151330`), and an offset written as a separate word
(`2024-06-15 13:30:00 +0800`, which GNU accepts).

### Debug tracing

`-D` categories other than `help` print a placeholder line and `-D help`
documents that honestly. Implementing `tree`, `opt`, `stat` and `exec` tracing
means threading a debug channel through both engines and the action sinks.

### Windows

- `EntryContext::physical_kind` ignores the directory-entry type hint on
  Windows, so every entry costs a metadata read that the hint could answer.
  The reparse-point check still needs the view, so the win is smaller than on
  Unix, but the hint could short-circuit non-reparse cases.
- `-ls` keeps its own printable-subset escaping rather than GNU's byte rule.
