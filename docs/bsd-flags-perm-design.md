# BSD `-flags` and `-perm` design

This document records the audit of the current implementation and defines the
next compatibility slice for macOS, FreeBSD, OpenBSD, NetBSD, and DragonFly
BSD. It is a design document only; implementation follows in separate commits.

## Findings from the current checkout

The current code already reads BSD `st_flags` from the `Metadata` used to build
the entry view, so matching does not need another metadata syscall. The main
problems are the flag vocabulary, syntax selection, and matcher algebra.

### `-flags` vocabulary is incomplete

`src/platform/unix/bsd.rs` exposes only:

- `arch`
- `nodump`
- `uchg`

The macOS `chflags(1)` vocabulary includes aliases and additional flags such
as `archived`, `opaque`, `sappnd`/`sappend`, `schg`/`schange`/`simmutable`,
`uappnd`/`uappend`, `uchange`/`uimmutable`, and `hidden`. FreeBSD adds a wider
set including `snapshot`, `sunlnk`/`sunlink`, `uarch`/`uarchive`,
`uhidden`/`hidden`, `uoffline`/`offline`, `urdonly`/`rdonly`/`readonly`,
`usparse`/`sparse`, `usystem`/`system`, `ureparse`/`reparse`, and
`uunlnk`/`uunlink`. NetBSD and OpenBSD have smaller, different vocabularies.

One shared BSD table cannot be exact on all targets. A name accepted on
FreeBSD may be invalid on NetBSD or OpenBSD, and macOS-only attributes such as
`hidden` must not be advertised on targets that do not define them.

### Flag aliases and clear forms are not modeled

The current parser recognizes a `no` prefix generically, except for the literal
`nodump`. BSD `chflags` semantics instead define aliases and clear forms as
part of the platform vocabulary. For example, `dump` clears `nodump`, while
`nonodump` is invalid on macOS. Aliases such as `archived` and `uimmutable`
must map to the same bit as their canonical spelling.

The matcher should retain the semantic operation represented by each token:
set this bit or require this bit to be clear. A string containing both
`arch` and `noarch` is not a parser contradiction on the BSD implementations
checked here; it is an unsatisfiable expression that evaluates false.

### Exact matching ignores unlisted bits

`FileFlagsMatcher::Exact` currently computes a `universe_mask` from the known
`FlagSpec` entries and compares only those bits. If a file has an unlisted bit,
an exact query can match incorrectly. On macOS, setting the `hidden` flag on a
file demonstrates this: `find -flags noarch` does not match the hidden file,
while the current three-entry table can report a match because it ignores the
hidden bit.

The exact matcher must compare every flag bit represented by the active
platform backend. Unknown/reserved bits should not be silently discarded. The
platform table therefore needs a complete comparison mask, separate from the
set of accepted aliases.

### `-flags` prefix syntax differs by BSD

The manuals do not define one common prefix grammar:

- macOS and FreeBSD support no prefix (exact), `-` (all conditions), and `+`
  (any condition).
- OpenBSD and NetBSD document no prefix (exact) and `-` (all); `+` is not a
  documented `-flags` mode.
- NetBSD additionally documents the special operand `none` for files with no
  flag bits.

The parser currently accepts `+` and `-` for every platform and has no `none`
representation. Platform syntax must be selected during planning, after the
active backend is known, while preserving the raw operand in the AST.

### `-perm` accepts the wrong dialect everywhere

The current parser implements the GNU shape:

- no prefix: exact
- `-`: all requested mode bits
- `/`: any requested mode bit
- numeric `+`: rejected

BSD differs:

- macOS and FreeBSD accept numeric `+mode` as any matching mode bit. This is a
  compatible extension that should also be accepted on Linux; GNU `/mode`
  remains available alongside it.
- macOS/OpenBSD/NetBSD document `-mode` as the all-bits form and do not
  document GNU `/mode`; macOS rejects `/644`, but the project superset may keep
  `/mode` where it has no conflicting meaning.
- A symbolic operand beginning with `+`, such as `+t` or `+X`, is interpreted
  through the symbolic-mode grammar rather than as the numeric any prefix.
  Numeric `+644` is the disambiguated BSD any form.

The parser must distinguish numeric `+` from symbolic `+`. The shared superset
should accept both `/mode` and numeric `+mode` as any-bit forms on Unix targets
with mode bits. This does not replace `/mode`, and it does not change symbolic
`+t` or `+X` parsing. A platform switch is needed only if a target gives one of
these spellings a conflicting meaning.

### Invalid octal modes are silently truncated

`parse_octal_mask` currently parses any octal integer and then masks it with
`07777`. Values such as `10000` therefore become zero and are accepted. GNU
find and the BSD `find` implementations reject modes containing bits outside
`07777`. Validation must happen before masking.

## Superset-first implementation policy

Do not introduce a target-specific parser dialect as the first abstraction.
Extend the shared parser and matcher to accept the union of compatible GNU and
BSD forms:

- numeric `+mode` is any-bit matching wherever mode bits are supported;
- GNU `/mode` remains any-bit matching wherever it already works;
- `-flags +NAME` is accepted wherever file flags are supported;
- `none` is accepted when the backend can provide a complete comparison mask;
- aliases that map to one native bit share one matcher condition.

Only split behavior by platform after proving that one spelling cannot have one
coherent meaning. The platform backend remains responsible for native flag
values, complete comparison coverage, and unsupported-metadata capability
checks; the parser should not branch on operating-system names merely because a
native manual omits a spelling.

### BSD flag tables

Use target-specific tables with canonical bits and aliases:

- macOS: `arch`/`archived`, `nodump` plus `dump`, `opaque`, `sappnd`/`sappend`,
  `schg`/`schange`/`simmutable`, `uappnd`/`uappend`, `uchg`/`uchange`/
  `uimmutable`, and `hidden`.
- FreeBSD: the full `chflags(1)` table, including system/user undeletable,
  archive, hidden, offline, readonly, sparse, system, and reparse aliases.
- NetBSD: `arch`, `opaque`, `nodump`, `sappnd`, `schg`, `uappnd`, and `uchg`,
  plus the documented `no` clear forms and `none` operand.
- OpenBSD: `arch`, `nodump`, `sappnd`, `schg`, `uappnd`, and `uchg`; `arch` is
  compatibility-only on that platform but remains a valid name.
- DragonFly BSD: use its native `chflags` vocabulary after checking the target
  headers rather than inheriting the FreeBSD table blindly.

Where libc exposes aliases inconsistently, define the numeric constants in the
target backend from the OS headers and keep the public spelling table beside
those constants. Do not use a Linux flag value as a BSD fallback. The table is
platform data; acceptance of compatible syntax remains shared.

### Matcher algebra

Replace the current exact-mode implementation with explicit masks:

- `set_mask`: bits required to be set.
- `clear_mask`: bits required to be clear.
- `comparison_mask`: all bits participating in exact comparison.

Evaluation becomes:

- exact: `(observed & comparison_mask) == set_mask` and no `clear_mask` bit is
  set; bits outside `comparison_mask` are treated according to the backend's
  declared comparison policy and are never silently ignored.
- all: every set condition is set and every clear condition is clear.
- any: at least one condition is satisfied, matching macOS/FreeBSD `+` rules.

Repeated identical conditions remain harmless. Opposite conditions are retained
and evaluate false instead of producing a planning error, matching the BSD
behavior observed on macOS.

`none` lowers to an exact zero-flags matcher and is only accepted when the
active flag dialect advertises it.

### Permission parsing

Change `parse_perm_argument` so its shared grammar accepts the superset:

1. Validate the operand as ASCII mode syntax.
2. If it begins with `-`, select all-bits mode.
3. If it begins with `/`, select any-bits mode.
4. If it begins with `+` and the remainder is all octal digits, select any-bits
   mode.
5. If it begins with `+` and is symbolic, parse it as the existing symbolic
   operation grammar, preserving the BSD ambiguity behavior.
6. Reject an octal value greater than `07777` before constructing the matcher.

The symbolic resolver remains shared. Its starting mode is zero, omitted `who`
continues to mean all classes for exact symbolic forms, and conditional `X`
continues to inspect the current mode accumulated within the symbolic operand.

## Planner and API changes

- Keep `Predicate::Flags(OsString)` and `Predicate::Perm(OsString)` unchanged
  in the AST so parsing does not depend on the host platform.
- Keep native flag tables and comparison masks in the platform backend. Do not
  add a parser-facing dialect object until a real semantic conflict requires
  one.
- Keep `parse_flags_argument` and `parse_perm_argument` shared for all
  compatible syntax; `planner::lower_metadata_predicate` supplies the active
  platform's native flag data and capability checks.
- Keep `PlatformFeature::FileFlags` and `PlatformFeature::ModeBits` as the
  capability gates. Unsupported platforms still fail during planning with the
  existing explicit diagnostics.
- Preserve Linux GNU behavior while adding the compatible superset: `/mode`
  remains available and numeric `+mode` becomes an any-bit alias.
- Preserve Windows behavior for its native attribute flag names and mode-bit
  rejection.

## Test matrix

### Shared unit tests

- Flag aliases map to one bit.
- `dump` maps to clear `nodump`; `nonodump` is rejected where the backend does
  not advertise it.
- Repeated and opposite conditions evaluate correctly without a parser-level
  contradiction error.
- Exact matching includes additional active-platform bits.
- `none` is accepted wherever the backend can provide complete flag coverage;
  acceptance is not restricted by target name.
- Octal modes above `07777` are rejected.
- Numeric `+644` and GNU `/644` both select any-bit matching on Linux and BSD.
- Symbolic `+t`, `+X`, `-u=X`, `g=u`, and `u=` preserve existing behavior.

### macOS integration tests

Use `chflags` to set and clear `hidden`, `opaque`, `uchg`, and `nodump` on a
temporary file. Compare `rfd` with `/usr/bin/find` for exact, all, any, alias,
clear, and mixed-condition operands. Include an exact query while an otherwise
unlisted active bit is set.

### FreeBSD/OpenBSD/NetBSD integration tests

Run the same table-driven cases against the native `find(1)` and `chflags(1)`
vocabularies. Keep expected accepted-name sets per operating system; do not
assume FreeBSD aliases exist on NetBSD or OpenBSD. Include NetBSD `-flags none`
and verify that undocumented `+` flag mode is rejected where the native tool
rejects it.

### GNU regression tests

The existing Linux GNU differential tests remain authoritative for `/mode`,
exact/all matching, symbolic mode resolution, and invalid numeric forms. Add
explicit coverage for rejecting `10000` while accepting `+644` as the shared
any-bit superset, plus coverage for genuinely incompatible flag aliases.

## Implementation order

1. Fix `-perm` octal validation and accept numeric `+mode` as the shared any-bit
   superset alongside `/mode`.
2. Expand BSD flag tables with native aliases and clear-form parsing.
3. Correct exact flag matching and remove parser-level contradictory-condition
   rejection.
4. Add macOS differential coverage, then FreeBSD/OpenBSD/NetBSD/DragonFly
   target validation.

Priority 1 features such as `-s` and `-acl` remain deferred.
