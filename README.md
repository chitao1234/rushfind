# rushfind

Find for the occupātus.

`rushfind` is a Rust implementation of Unix `find` that targets GNU `find` syntax while adding a parallel traversal engine. The installed binary is `rfd`.

## Help and manpage

`rfd --help` is the short terminal reference. The fuller reference is the
`rfd(1)` manpage.

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
- GNU normal compatibility options: `-files0-from FILE`, `-noleaf`, `-warn`, `-nowarn`,
  `-ignore_readdir_race`, and `-noignore_readdir_race`
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
- `-delete` implies depth-mode traversal, so directories are evaluated and removed after their
  scheduled descendants
- In depth mode, `-prune` remains boolean-true in expression flow but does not block descendant
  traversal
- `-fstype` uses the active platform mount snapshot backend and stays exact on Linux, macOS, the
  supported BSD targets, and Windows
- `-Olevel` is accepted for GNU command-line compatibility but does not change optimizer behavior
- `-D debugopts` emits lightweight `rushfind` diagnostics for requested categories rather than
  GNU findutils' detailed tracing stream
- `-ignore_readdir_race` and `-noignore_readdir_race` are accepted and recorded compatibility
  options; this implementation does not yet alter runtime race handling for disappearing entries
- Access predicates use kernel access checks and intentionally can differ from `-perm`
- Access predicates use real-ID GNU `access(2)` semantics and are not mode-bit emulation
- `-xdev` and `-mount` are normalized as traversal-wide structural limits in the current
  implementation rather than GNU-style positional controls
- `-newerXY` supports exact Unix-family birth-time forms where the active backend exposes birth
  time, plus a strict literal-time subset:
  `@<unix-seconds>[.frac]`, `YYYY-MM-DD`, and `YYYY-MM-DD[ T]HH:MM[:SS][.frac][Z|±HH[:MM]]`

## Platform scope

- Linux remains the reference platform and keeps the strongest GNU compatibility coverage.
- macOS, FreeBSD, NetBSD, OpenBSD, and DragonFly BSD are supported through alternate Unix-family
  backends for filesystem, account, and locale behavior.
- illumos, Solaris, and Haiku are supported in the first generic Unix fallback tier.
- Native Windows is supported through a Windows backend for filesystem, account, locale, path,
  and access behavior.
- `rushfind` prefers exact GNU-compatible behavior on non-Linux Unix when the host exposes the
  needed primitive through another code path.
- Interactive locale handling for `-ok` and `-okdir` remains approximate on non-Linux Unix and
  emits a startup warning when planned.
- During the initial macOS port, case-insensitive glob matching may still differ outside the C
  locale and emits a startup warning when planned.
- The generic Unix tier keeps `-xdev` / `-mount`, ownership predicates, access predicates, mode
  bits, `-ls`, and the common print / exec surfaces working through shared Unix code.
- On the generic Unix tier, `-fstype`, `%F`, `-flags`, and birth-time predicates / `%B*` fail
  during planning with explicit diagnostics instead of panicking.
- On the generic Unix tier, interactive locale handling and case-insensitive glob matching remain
  approximate and emit startup warnings when planned.
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
- On Windows, interactive locale handling and case-insensitive glob matching remain approximate
  and emit startup warnings when planned.

## Worker selection

The `rfd` binary keeps the command-line syntax identical to GNU `find`.

Use the `RUSHFIND_WORKERS` environment variable to control execution mode:

- `RUSHFIND_WORKERS=1` keeps traversal/output close to GNU ordering
- `RUSHFIND_WORKERS=4` enables the worker-owned relaxed-order parallel engine by default

## Follow modes

- `-P` keeps physical traversal semantics and is the default
- `-H` follows symlinks only for command-line roots
- `-L` follows symlinks logically during traversal
- Followed-directory traversal is loop-safe and reports a runtime error instead of recursing forever

## Unsupported currently

The current implementation supports `-exec ... ;`, `-exec ... +`, `-execdir ... ;`,
`-execdir ... +`, `-ok ... ;`, `-okdir ... ;`, and `-delete`.

`-ok ... +` and `-okdir ... +` remain unsupported.

## Development

Contributor-facing documentation lives in [`docs/development.md`](docs/development.md).
