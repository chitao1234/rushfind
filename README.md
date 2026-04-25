# rushfind

Find for the occupātus.

`rushfind` is a Rust implementation of Unix `find` that targets GNU `find` syntax while adding a parallel traversal engine. The installed binary is `rfd`.

## Command documentation

- Use `rfd --help` for a compact interactive quick reference.
- Use `rfd -D help` for debug diagnostic categories.
- Use `man ./doc/rfd.1` from a source checkout for the full first-party command reference.
- Regenerate the checked-in manpage after editing `doc/rfd.1.scd` with:

```bash
scripts/generate_manpage.sh
```

The `scdoc` tool is only needed by maintainers regenerating `doc/rfd.1`; normal
`cargo build` does not depend on it.

## License

`rushfind` is licensed under either of the following, at your option:

- MIT
- Apache-2.0

## Current scope

- GNU-style argv parsing, including comma expression sequencing
- Global follow-mode and normal compatibility options: `-P`, `-H`, `-L`, `-version`,
  `--version`, `--help`, `-Olevel`, `-D debugopts`
- Read-only predicates: `-name`, `-iname`, `-path`, `-ipath`, `-type`, `-xtype`, `-true`,
  `-false`, `-fstype`
- `-type` and `-xtype` accept GNU-style comma lists such as `-type f,d`
- GNU normal and positional compatibility options: `-files0-from FILE`, `-follow`, `-noleaf`,
  `-warn`, `-nowarn`, `-ignore_readdir_race`, and `-noignore_readdir_race`
- Identity/link predicates: `-samefile`, `-inum`, `-links`
- Ownership/account predicates: `-uid`, `-gid`, `-user`, `-group`, `-nouser`, `-nogroup`,
  plus Windows-specific `-owner`, `-owner-sid`, and `-group-sid`
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
  `%l`, `%i`, `%n`, `%D`, `%b`, `%k`, `%u`, `%U`, `%US`, `%g`, `%G`, `%GS`, `%F`, `%a`, `%c`,
  `%t`, `%B`, `%A*`, `%C*`, `%T*`, `%B*`, `%%`, `\\`, `\n`, `\t`, and `\0`
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
- `-Olevel` is accepted for GNU command-line compatibility but does not change optimizer behavior
- `-D debugopts` emits lightweight `rushfind` diagnostics for requested categories rather than
  GNU findutils' detailed tracing stream
- `-ignore_readdir_race` and `-noignore_readdir_race` are accepted and recorded compatibility
  options; this implementation does not yet alter runtime race handling for disappearing entries
- `-context`, `-type D`, and `-xtype D` are recognized for GNU compatibility but fail with
  explicit unsupported diagnostics unless SELinux label matching or Solaris door matching is added
  in a future platform-specific slice
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
- illumos, Solaris, and Haiku are supported in the first generic Unix fallback tier.
- Native Windows is supported through a Windows backend for filesystem, account, locale, path,
  and access behavior.
- macOS CI uses a cached source build of pinned GNU findutils revisions so GNU differential
  coverage does not depend on the runner image or Homebrew's package freshness.
- Native Windows CI exercises both `x86_64-pc-windows-gnu` and `x86_64-pc-windows-msvc`.
- `rushfind` prefers exact GNU-compatible behavior on non-Linux Unix when the host exposes the
  needed primitive through another code path.
- `LC_CTYPE` is resolved from `LC_ALL`, `LC_CTYPE`, then `LANG`. `C` and `POSIX` use
  byte-oriented matching; UTF-8 and supported legacy encodings use crate-backed character
  decoding for glob and GNU regex predicates.
- `LC_CTYPE` support is owned by `rushfind` rather than delegated to libc locale APIs. Unknown
  encodings warn only when locale-sensitive matching or presentation is planned.
- `LC_COLLATE` ordering, collating symbols, equivalence classes, and user-defined locale
  tailoring remain out of scope.
- Case-insensitive matching uses single-character folding. Multi-character folds such as `ß`
  matching `ss` are intentionally not supported.
- `LC_MESSAGES` may select localized `-ok` / `-okdir` prompt fragments where available. Prompt
  replies use a built-in ASCII affirmative parser, so `LC_CTYPE` does not change confirmation
  parsing.
- The generic Unix tier keeps `-xdev` / `-mount`, ownership predicates, access predicates, mode
  bits, `-ls`, and the common print / exec surfaces working through shared Unix code.
- The generic Unix tier does not claim GNU differential parity.
- On the generic Unix tier, `-fstype`, `%F`, `-flags`, and birth-time predicates / `%B*` fail
  during planning with explicit diagnostics instead of panicking.
- On Windows, name and path matching accept both `/` and `\` as separators, and displayed paths
  render with backslashes.
- On Windows, `-user`, `-group`, `-nouser`, `-nogroup`, `%u`, `%g`, `%US`, `%GS`, `-readable`,
  `-writable`, `-executable`, `-fstype`, `-xdev`, `-mount`, `-ls`, `-fls`, and the `-exec*` /
  `-ok*` family use native Windows contracts.
- On Windows, `-owner NAME`, `-owner-sid SID`, and `-group-sid SID` provide native ownership
  matching. `-owner` matches the file owner by account name, while `-owner-sid` and
  `-group-sid` match raw owner and group SIDs exactly. `%u` and `%g` render names first and
  fall back to SID text when lookup fails, while `%US` and `%GS` render canonical owner and
  group SID text explicitly.
- On Windows, `-ls` and `-fls` render the native record shape
  `fileid alloc-kib type+attrs links owner size mtime pathname`.
- `-flags` is cross-platform, symbolic-only, and accepts only the host-native flag names that
  the active build supports.
- On Windows, `-reparse-type` classifies reparse points by semantic class such as `symbolic` and
  `mount-point`.
- On Windows, `-uid`, `-gid`, `-perm`, `%U`, `%G`, `%m`, `%M`, `%D`, `%b`, and `%k` fail during
  planning with explicit diagnostics. When raw Windows principal matching is needed, use
  `-owner-sid` or `-group-sid`.

## Manual verification

Build the binary and run the Unix-family portability smoke harness locally, then repeat it on
target hosts:

The minimum supported Rust version is 1.85.0.

```bash
cargo build
bash scripts/check_unix_portability_surface.sh target/debug/rfd
bash scripts/check_generic_unix_target_builds.sh
```

The Unix-family smoke harness exercises `-version`, `-print`, `-print0`, optional `-fstype`
and birth-time probes, `-xdev`, ownership/access rendering, `-ls`, and `-execdir`. It also prints
the locale-sensitive `-ok` commands to run manually on the target host.

For a compile-only generic Unix preflight from a development host, use:

```bash
bash scripts/check_generic_unix_target_builds.sh
```

The default target list is:

- `x86_64-unknown-illumos`
- `x86_64-pc-solaris`
- `x86_64-unknown-haiku`

The helper skips targets whose `rust-std` component is not shipped by the selected toolchain, or
whose target C toolchain is not configured for `pcre2-sys`. On this Linux-host cross-preflight
path, that currently means Haiku may need native-host validation because Rust 1.85 does not ship
its `rust-std`, while illumos and Solaris may need explicit cross-compiler setup before the helper
can exercise them from Linux.

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
