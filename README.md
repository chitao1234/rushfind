# rushfind

Find for the occupātus.

`rushfind` is a Rust implementation of Unix `find` that targets GNU `find` syntax while adding a parallel traversal engine. The installed binary is `rfd`.

## License

`rushfind` is licensed under either of the following, at your option:

- MIT
- Apache-2.0

## Current scope

- GNU-style argv parsing
- Global follow-mode options: `-P`, `-H`, `-L`
- Read-only predicates: `-name`, `-iname`, `-path`, `-ipath`, `-type`, `-xtype`, `-true`,
  `-false`, `-fstype`
- Identity/link predicates: `-samefile`, `-inum`, `-links`
- Ownership/account predicates: `-uid`, `-gid`, `-user`, `-group`, `-nouser`, `-nogroup`
- Permission/access predicates: `-perm`, `-readable`, `-writable`, `-executable`
- Size/time predicates: `-size`, `-empty`, `-used`, `-mtime`, `-atime`, `-ctime`, `-mmin`,
  `-amin`, `-cmin`, `-newer`, `-anewer`, `-cnewer`, full read-only `-newerXY`, `-daystart`
- Time predicates and `-used` accept GNU-style fractional magnitudes such as `0.5`, `+1.25`,
  and `-0.75`
- Symlink-content predicates: `-lname`, `-ilname`
- Traversal controls: `-mindepth`, `-maxdepth`, `-depth`, `-prune`, `-xdev`, `-mount`
- Output and mutation actions: `-print`, `-print0`, `-printf`, `-fprint`, `-fprint0`,
  `-fprintf`, `-ls`, `-fls`, `-exec ... ;`, `-exec ... +`, `-execdir ... ;`,
  `-execdir ... +`, `-ok ... ;`, `-okdir ... ;`, `-delete`, `-quit`
- `-printf` currently supports `%p`, `%P`, `%H`, `%f`, `%h`, `%d`, `%y`, `%s`, `%m`, `%M`,
  `%l`, `%i`, `%n`, `%D`, `%b`, `%k`, `%u`, `%U`, `%g`, `%G`, `%F`, `%a`, `%c`, `%t`, `%B`,
  `%A*`, `%C*`, `%T*`, `%B*`, `%%`, `\\`, `\n`, `\t`, and `\0`
- Supported `-printf` directives accept GNU-style field formatting with
  `%[flags][width][.precision]directive`
- Time-oriented `-printf` directives render in the process local timezone while freezing textual
  names to C-locale spellings in the current implementation
- GNU special time selectors `%A@`, `%C@`, `%T@`, `%B@`, `%A+`, `%C+`, `%T+`, and `%B+` are
  supported
- Unsupported `-printf` directives fail during planning with explicit diagnostics
- Birth-time `-printf` directives and `B`-time predicates use exact Unix-family backend reads when
  the host exposes birth time, even on hosts where local GNU `find` does not expose equivalent
  `%B*` output
- Ordered single-worker mode stays GNU-oriented and remains a separate engine for supported
  structural traversal controls
- Ordered single-worker mode renders GNU-shaped `-ls` / `-fls` records on Unix-family hosts and
  native Windows attribute records on Windows, with a frozen evaluation timestamp so recent-time
  classification stays deterministic within a run
- Ordered single-worker mode matches GNU `-quit` behavior for the supported action set
- File-backed print actions eagerly create or truncate their destinations at startup, even when
  the action is never reached dynamically or no entry matches
- Relaxed-order parallel mode is subtree-scheduled and worker-owned, may emit side effects out of
  order, guarantees prune subtree boundaries in pre-order traversal, and does not promise GNU
  sibling ordering
- Relaxed-order parallel `-fprint*` writes are atomic per destination file but do not promise
  traversal order within that file
- Relaxed-order parallel `-ls` stdout emission and `-fls` destination writes are atomic per
  rendered record but do not promise GNU sibling order
- Relaxed-order parallel mode treats `-quit` as cancellation: no new subtree tasks are published
  after it is observed, already granted work may still finish, and buffered `-exec ... +` and
  `-execdir ... +` batches still flush
- Ordered single-worker mode inherits child stdio for `-exec` and `-execdir`
- Relaxed-order parallel mode buffers child stdout/stderr for atomic replay for `-exec` and
  `-execdir`
- `-execdir` uses `./basename` on Unix-family hosts and `.\basename` on Windows, and rejects
  unsafe `PATH` entries eagerly before traversal begins
- `-delete` implies depth-mode traversal, so directories are evaluated and removed after their
  scheduled descendants
- In depth mode, `-prune` remains boolean-true in expression flow but does not block descendant
  traversal
- Relaxed-order parallel mode preserves descendant-before-parent completion for depth-mode actions
- `-fstype` uses the active platform mount snapshot backend and stays exact on Linux, macOS, the
  supported BSD targets, and Windows
- `-fstype` type names come from `/proc/self/mountinfo` on Linux, `getmntinfo` snapshots on
  macOS and BSD, and volume metadata on Windows
- Requested filesystem types are resolved against the set known at command startup
- Commands that do not use `-fstype` do not read mount-table state
- Access predicates use kernel access checks and intentionally can differ from `-perm`
- Access predicates use real-ID GNU `access(2)` semantics and are not mode-bit emulation
- When available, the access predicate path uses `faccessat`, with `access(2)` as the fallback
- `-xdev` and `-mount` are normalized as traversal-wide structural limits in the current
  implementation rather than GNU-style positional controls
- Internal performance substrate: lazy entry data access and cheap-first planning for pure read-only `-a` chains
- `-newerXY` supports exact Unix-family birth-time forms where the active backend exposes birth
  time, plus a strict literal-time subset:
  `@<unix-seconds>[.frac]`, `YYYY-MM-DD`, and `YYYY-MM-DD[ T]HH:MM[:SS][.frac][Z|±HH[:MM]]`
- Installed GNU `find` builds can still reject `B` predicates on hosts where GNU findutils does
  not expose birth-time support; `rushfind` keeps `B` handling enabled when the active Unix-family
  backend can read birth time

## Platform scope

- Linux remains the reference platform and keeps the strongest GNU compatibility coverage.
- macOS, FreeBSD, NetBSD, OpenBSD, and DragonFly BSD are supported through alternate Unix-family
  backends for filesystem, account, and locale behavior.
- Native Windows is supported through a Windows backend for filesystem, account, locale, path,
  and access behavior.
- macOS CI uses a cached source build of pinned GNU findutils revisions so GNU differential
  coverage does not depend on the runner image or Homebrew's package freshness.
- Native Windows CI exercises both `x86_64-pc-windows-gnu` and `x86_64-pc-windows-msvc`.
- `rushfind` prefers exact GNU-compatible behavior on non-Linux Unix when the host exposes the
  needed primitive through another code path.
- Interactive locale handling for `-ok` and `-okdir` remains approximate on non-Linux Unix and
  emits a startup warning when planned.
- During the initial macOS port, case-insensitive glob matching may still differ outside the C
  locale and emits a startup warning when planned.
- On Windows, name and path matching accept both `/` and `\` as separators, and displayed paths
  render with backslashes.
- On Windows, `-user`, `-group`, `-nouser`, `-nogroup`, `%u`, `%g`, `-readable`, `-writable`,
  `-executable`, `-fstype`, `-xdev`, `-mount`, `-ls`, `-fls`, and the `-exec*` / `-ok*` family
  use native Windows contracts.
- On Windows, `-uid`, `-gid`, `-perm`, `%U`, `%G`, `%m`, `%M`, `%D`, `%b`, and `%k` fail during
  planning with explicit diagnostics.
- On Windows, interactive locale handling and case-insensitive glob matching remain approximate
  and emit startup warnings when planned.

## Manual verification

Build the binary and run the Unix-family portability smoke harness locally, then repeat it on
target hosts:

The minimum supported Rust version is 1.85.0.

```bash
cargo build
bash scripts/check_unix_portability_surface.sh target/debug/rfd
```

The script exercises `-print`, `-print0`, `-fstype`, birth-time reads, `-xdev`, ownership/access
rendering, `-ls`, and `-execdir`. It also prints the locale-sensitive `-ok` commands to run
manually on the target host.

For a non-Windows preflight of the Windows code path, use:

```bash
cargo +1.85.0 check --tests --target x86_64-pc-windows-gnu
```

On native Windows hosts, the CI matrix covers:

```powershell
cargo test --target x86_64-pc-windows-msvc
cargo test --target x86_64-pc-windows-gnu
```

## Worker selection

The `rfd` binary keeps the command-line syntax identical to GNU `find`.

Use the `RUSHFIND_WORKERS` environment variable to control execution mode:

- `RUSHFIND_WORKERS=1` keeps traversal/output close to GNU ordering
- `RUSHFIND_WORKERS=4` enables the worker-owned relaxed-order parallel engine by default

## Current regex benchmark harness

Use the regex benchmark harness when you want end-to-end ordered versus parallel comparisons on
regex-heavy workloads:

```bash
RUSHFIND_WORKERS=8 RUSHFIND_BENCH_REPEATS=5 bash scripts/bench_regex_stage.sh cd95653
```

The script builds baseline and current release binaries outside the timed region, reuses one
deterministic fixture tree for both trees, and prints per-case median deltas for regex-light,
regex-heavy, and PCRE2-fallback-heavy command families.

## Follow modes

- `-P` keeps physical traversal semantics and is the default
- `-H` follows symlinks only for command-line roots
- `-L` follows symlinks logically during traversal
- Followed-directory traversal is loop-safe and reports a runtime error instead of recursing forever

## Unsupported currently

The current implementation supports `-exec ... ;`, `-exec ... +`, `-execdir ... ;`,
`-execdir ... +`, `-ok ... ;`, `-okdir ... ;`, and `-delete`.

`-ok ... +` and `-okdir ... +` remain unsupported.
