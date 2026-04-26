# Development

This document holds contributor-facing notes that were extracted from the
repository README so the top-level project page can stay focused on end users.

## Manpage authoring

`rfd --help` is the short terminal reference. The fuller manual source lives at
`docs/rfd.1.scd`.

Regenerate the checked-in manpage after editing `docs/rfd.1.scd` with:

```bash
scripts/generate_manpage.sh
```

Render and preview it locally with:

```bash
scdoc < docs/rfd.1.scd > docs/rfd.1
man -l docs/rfd.1
```

The `scdoc` tool is only needed when regenerating `docs/rfd.1`; normal
`cargo build` does not depend on it.

## Detailed implementation notes

These notes are intentionally more implementation-specific than the README.

- Ordered single-worker mode stays GNU-oriented and remains a separate engine
  for supported structural traversal controls.
- Ordered single-worker mode renders GNU-shaped `-ls` / `-fls` records on
  Unix-family hosts and native Windows attribute records on Windows, with a
  frozen evaluation timestamp so recent-time classification stays deterministic
  within a run.
- Ordered single-worker mode matches GNU `-quit` behavior for the supported
  action set.
- File-backed print actions eagerly create or truncate their destinations at
  startup, even when the action is never reached dynamically or no entry
  matches.
- Relaxed-order parallel mode is subtree-scheduled and worker-owned, may emit
  side effects out of order, guarantees prune subtree boundaries in pre-order
  traversal, and does not promise GNU sibling ordering.
- Relaxed-order parallel `-fprint*` writes are atomic per destination file but
  do not promise traversal order within that file.
- Relaxed-order parallel `-ls` stdout emission and `-fls` destination writes
  are atomic per rendered record but do not promise GNU sibling order.
- Relaxed-order parallel mode treats `-quit` as cancellation: no new subtree
  tasks are published after it is observed, already granted work may still
  finish, and buffered `-exec ... +` and `-execdir ... +` batches still flush.
- Ordered single-worker mode inherits child stdio for `-exec` and `-execdir`.
- Relaxed-order parallel mode buffers child stdout/stderr for atomic replay for
  `-exec` and `-execdir`.
- `-execdir` uses `./basename` on Unix-family hosts and `.\basename` on
  Windows, and rejects unsafe `PATH` entries eagerly before traversal begins.
- Relaxed-order parallel mode preserves descendant-before-parent completion for
  depth-mode actions.
- `-fstype` type names come from `/proc/self/mountinfo` on Linux,
  `getmntinfo` snapshots on macOS and BSD, and volume metadata on Windows.
- Requested filesystem types are resolved against the set known at command
  startup.
- Commands that do not use `-fstype` do not read mount-table state.
- When available, the access predicate path uses `faccessat`, with `access(2)`
  as the fallback.
- Internal performance substrate: lazy entry data access and cheap-first
  planning for pure read-only `-a` chains.
- Installed GNU `find` builds can still reject `B` predicates on hosts where
  GNU findutils does not expose birth-time support; `rushfind` keeps `B`
  handling enabled when the active Unix-family backend can read birth time.

## Platform development notes

- macOS CI uses a cached source build of pinned GNU findutils revisions so GNU
  differential coverage does not depend on the runner image or Homebrew's
  package freshness.
- Native Windows CI exercises both `x86_64-pc-windows-gnu` and
  `x86_64-pc-windows-msvc`.
- The generic Unix tier does not claim GNU differential parity.

## Manual verification

Build the binary and run the Unix-family portability smoke harness locally,
then repeat it on target hosts.

The minimum supported Rust version is `1.85.0`.

```bash
cargo build
bash scripts/check_unix_portability_surface.sh target/debug/rfd
bash scripts/check_generic_unix_target_builds.sh
```

The Unix-family smoke harness exercises `-version`, `-print`, `-print0`,
optional `-fstype` and birth-time probes, `-xdev`, ownership/access rendering,
`-ls`, and `-execdir`. It also prints the locale-sensitive `-ok` commands to
run manually on the target host.

For a compile-only generic Unix preflight from a development host, use:

```bash
bash scripts/check_generic_unix_target_builds.sh
```

The default target list is:

- `x86_64-unknown-illumos`
- `x86_64-pc-solaris`
- `x86_64-unknown-haiku`

The helper skips targets whose `rust-std` component is not shipped by the
selected toolchain, or whose target C toolchain is not configured for
`pcre2-sys`. On this Linux-host cross-preflight path, that currently means
Haiku may need native-host validation because Rust `1.85.0` does not ship its
`rust-std`, while illumos and Solaris may need explicit cross-compiler setup
before the helper can exercise them from Linux.

For a non-Windows preflight of the Windows code path, use:

```bash
cargo +1.85.0 check --tests --target x86_64-pc-windows-gnu
```

On native Windows hosts, the CI matrix covers:

```powershell
cargo test --target x86_64-pc-windows-msvc
cargo test --target x86_64-pc-windows-gnu
```

## Regex benchmark harness

Use the regex benchmark harness when you want end-to-end ordered versus
parallel comparisons on regex-heavy workloads:

```bash
RUSHFIND_WORKERS=8 RUSHFIND_BENCH_REPEATS=5 bash scripts/bench_regex_stage.sh cd95653
```

The script builds baseline and current release binaries outside the timed
region, reuses one deterministic fixture tree for both trees, and prints
per-case median deltas for regex-light, regex-heavy, and
PCRE2-fallback-heavy command families.
