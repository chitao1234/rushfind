# Find compatibility and performance backlog

This is a working backlog for extending `rfd` beyond its current GNU-focused
surface. It includes the BSD `find(1)` family (FreeBSD, OpenBSD, NetBSD, and
the closely related macOS behavior) and implementation issues visible in the
current traversal and evaluation pipeline.

## Current baseline

`cargo test --all-targets` passes all tests in the current checkout. The parser,
planner, evaluator, ordered walker, relaxed parallel walker, action pipeline,
locale handling, and Unix-family backends are covered well for the features
that are already implemented.

The following common BSD spellings are currently rejected as unsupported
expression tokens: `-E`, `-s`, `-x`, `-d`, `-X`, `-printx`, `-acl`, `-Bmin`,
`-asince`, `-rm`, `-exit`, and `-f path`.

## Priority 0: compatibility that users will hit immediately

### BSD option aliases and option placement

Add a BSD compatibility layer during leading-option parsing and preserve the
option state in `CommandAst`/`CompatibilityOptions`:

| BSD spelling | Meaning | Existing implementation to reuse |
| --- | --- | --- |
| `-d` | depth-first traversal | `Predicate::Depth` / `TraversalOrder::DepthFirstPostOrder` |
| `-x` | stay on the starting filesystem | `Predicate::XDev` / `same_file_system` |
| `-E` | use extended regular expressions for `-regex` and `-iregex` | `RegexDialect::PosixExtended` |
| `-f path` | add a root path, including paths beginning with `-`, `!`, or `(` | `CommandAst::start_paths` |
| OpenBSD `-h` | follow command-line symlinks | `FollowMode::CommandLineOnly` |

`-d` and `-x` are especially cheap aliases. `-E` must be represented as a
state change before regex primaries are lowered, just like positional
`-regextype`; it should not compile a regex by itself. `-f` needs parser support
because treating its operand as an ordinary path is not enough when the path
would otherwise be classified as an expression token.

### BSD time predicates

Implement the creation/birth-time family where the active backend advertises
`PlatformFeature::BirthTime`:

- FreeBSD/macOS: `-Bmin`, `-Btime`, `-Bnewer`.
- FreeBSD aliases: `-mnewer` (same comparison as `-newer`).
- NetBSD aliases: `-asince`, `-csince`, `-since`, and the corresponding
  `-newerat`, `-newerct`, `-newermt` forms.

The existing timestamp matcher and birth-time metadata accessor provide most
of the runtime machinery. The missing pieces are parser atoms, planner
lowering, and BSD-compatible duration parsing. BSD accepts compound units such
as `-1h30m` and parsed date strings; the current parser only accepts the GNU
numeric/fractional subset and a strict literal format for `-newerXY`.

### BSD output and mutation aliases

These are small, high-value additions:

- `-rm` as an alias for `-delete` (NetBSD).
- `-exit [status]` as an immediate exit action (NetBSD). The action needs an
  explicit exit status in the runtime control path; it is not equivalent to
  GNU `-quit`, which exits successfully after the current pipeline work.
- `-printx` (NetBSD), which emits xargs-quoted names.
- `-X` (FreeBSD/OpenBSD/NetBSD), which skips names unsafe for plain `xargs` and
  reports a diagnostic. This is a traversal/output policy, not a filename
  predicate.

The existing byte-preserving output and action broker are suitable for
`-printx`; add a dedicated renderer rather than passing through lossy UTF-8.

## Priority 1: BSD metadata and traversal behavior

### Sorted traversal (`-s`)

FreeBSD, NetBSD, and macOS provide `-s`, sorting entries within each directory
before descent. Add `TraversalOrder::Lexicographic` (or a separate
`sort_children` bit) and sort using the platform byte/locale policy already
used by path rendering. The sort must be per-directory; globally sorting the
final output is incorrect. In parallel mode, `-s` should force an ordered
execution policy unless a deterministic per-directory scheduling guarantee is
added.

### ACL predicate (`-acl`)

FreeBSD exposes `-acl` to select files with extended ACLs. Add an optional
`acl_present` field to `PlatformMetadataView` and a capability bit. The planner
should fail during planning on platforms without an ACL reader, following the
existing explicit-diagnostic pattern for unsupported metadata. Do not infer ACL
presence from mode bits.

### BSD `-flags` and `-perm` semantics

The shared flag and permission parsers cover much of the syntax, but BSD
`-flags` has different exact/all/any handling and native flag names. Add
platform-specific flag specs and differential tests for FreeBSD/macOS instead
of assuming the current Linux names and algebra are portable. BSD symbolic
`-perm` also has edge cases around omitted `who` and special bits that should be
checked against each target's `find(1)`.

## Priority 2: GNU behavior that is accepted but still only nominal

- `-ignore_readdir_race` and `-noignore_readdir_race` are parsed and stored,
  but disappearing directory entries still follow the normal error path. The
  option should suppress the specific ENOENT/ESTALE race diagnostics while
  preserving permission and I/O errors.
- `-Olevel` is accepted but does not select an optimizer strategy. Either make
  levels meaningful or document the accepted compatibility range as a no-op.
- `-D` categories currently print a placeholder saying detailed tracing is not
  implemented. Implement at least `tree`, `opt`, `stat`, and `exec` around the
  existing planner and runtime events, or narrow the advertised help.
- `-context` and Solaris door types are recognized but unsupported. These are
  correctly diagnosed; they should remain explicit platform slices rather than
  silently evaluating false.

## Performance issues to measure and resolve

### Ordered walker buffers every directory

`walk_ordered_with_backend` calls `read_children`, which collects the entire
directory into a `Vec<DiscoveredChild>` before pushing work. Peak memory is
therefore proportional to the widest directory, and the first child cannot be
processed until enumeration finishes. For unsorted pre-order traversal, stream
children directly onto the stack. Keep buffering only for post-order and
lexicographic modes where ordering requires it.

### Ancestry is cloned for every child

`PendingPath.ancestry` is a `Vec<FileIdentity>` and is cloned once per child in
both ordered and parallel traversal. This is O(number of entries × depth) copy
traffic and allocates heavily on deep or wide trees. Replace it with a shared
persistent ancestry node (`Arc` parent chain) or a traversal-local identity set
with scoped insert/remove operations.

### Scheduler polling adds avoidable latency

Workers that find no work wait on a condition variable with a 10 ms timeout and
then poll again. This creates a floor on wake-up latency and unnecessary wake
traffic on short jobs. Use a generation counter or an atomic outstanding-work
state paired with the condition variable so workers sleep until a real enqueue,
quit, or completion event.

### Directory entry type hints are not always enough

`visit_children` obtains `DirEntry::file_type()` and later metadata access may
still call `stat`/`lstat`. On filesystems with unknown `d_type`, this becomes an
extra syscall per entry. Measure syscall counts on ext4, NFS, and BSD UFS/ZFS;
where safe, carry a complete metadata result or avoid repeating the type read.

### Output and action serialization need profiling

Parallel workers serialize output through the broker and file-output locks. This
is correct for ordering and shared destinations, but high-volume `-print0` and
`-printf` workloads may become single-consumer bound. Add counters for queue
wait time, bytes per broker message, and file-output lock contention before
changing the design.

## Suggested implementation sequence

1. Add `-d`, `-x`, BSD `-E`, OpenBSD `-h`, and `-f path` parsing with parser and
   planner tests.
2. Add `-rm`, `-printx`, `-X`, and NetBSD `-exit` with output/action tests.
3. Add BSD birth-time aliases and duration/date parsing, gated by capabilities.
4. Add per-directory sorted traversal and force ordered execution for `-s`.
5. Implement race-option behavior and replace the 10 ms scheduler polling.
6. Measure and then refactor ancestry sharing and ordered-directory buffering.
7. Add ACL metadata only after selecting a portable backend contract for the
   supported BSD targets.

