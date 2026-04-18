# findoxide

`findoxide` is a fresh Rust implementation of Unix `find` that targets GNU `find` syntax while adding a parallel traversal engine.

## v0 and stage-14 scope

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
- Output and mutation actions: `-print`, `-print0`, `-printf`, `-exec ... ;`, `-exec ... +`,
  `-delete`, `-quit`
- `-printf` currently supports `%p`, `%P`, `%f`, `%h`, `%d`, `%y`, `%s`, `%m`, `%l`, `%%`, `\\`,
  `\n`, `\t`, and `\0`
- Unsupported `-printf` directives fail during planning with explicit diagnostics
- Ordered single-worker mode stays GNU-oriented and remains a separate engine for supported
  structural traversal controls
- Ordered single-worker mode matches GNU `-quit` behavior for the supported action set
- Relaxed-order parallel mode is subtree-scheduled and worker-owned, may emit side effects out of
  order, guarantees prune subtree boundaries in pre-order traversal, and does not promise GNU
  sibling ordering
- Relaxed-order parallel mode treats `-quit` as cancellation: no new subtree tasks are published
  after it is observed, already granted work may still finish, and buffered `-exec ... +` batches
  still flush
- Ordered single-worker mode inherits child stdio for `-exec`
- Relaxed-order parallel mode buffers child stdout/stderr for atomic replay
- `-delete` implies depth-mode traversal, so directories are evaluated and removed after their
  scheduled descendants
- In depth mode, `-prune` remains boolean-true in expression flow but does not block descendant
  traversal
- Relaxed-order parallel mode preserves descendant-before-parent completion for depth-mode actions
- `-fstype` is Linux-first in this stage
- `-fstype` type names come from `/proc/self/mountinfo`
- Requested filesystem types are resolved against the set known at command startup
- Commands that do not use `-fstype` do not read mount-table state
- Access predicates use kernel access checks and intentionally can differ from `-perm`
- Access predicates use real-ID GNU `access(2)` semantics and are not mode-bit emulation
- When available, the access predicate path uses `faccessat`, with `access(2)` as the fallback
- `-xdev` and `-mount` are normalized as traversal-wide structural limits in this stage rather
  than GNU-style positional controls
- Internal performance substrate: lazy entry data access and cheap-first planning for pure read-only `-a` chains
- `-newerXY` supports Linux-first birth-time forms and a strict literal-time subset:
  `@<unix-seconds>[.frac]`, `YYYY-MM-DD`, and `YYYY-MM-DD[ T]HH:MM[:SS][.frac][Z|±HH[:MM]]`
- Installed GNU `find` builds can still reject `B` predicates on hosts where GNU findutils does
  not expose birth-time support; the current implementation keeps Linux-first `B` handling in
  `findoxide`

## Worker selection

`findoxide` keeps the command-line syntax identical to GNU `find`.

Use the `FINDOXIDE_WORKERS` environment variable to control execution mode:

- `FINDOXIDE_WORKERS=1` keeps traversal/output close to GNU ordering
- `FINDOXIDE_WORKERS=4` enables the worker-owned relaxed-order parallel engine by default

## Follow modes

- `-P` keeps physical traversal semantics and is the default
- `-H` follows symlinks only for command-line roots
- `-L` follows symlinks logically during traversal
- Followed-directory traversal is loop-safe and reports a runtime error instead of recursing forever

## Unsupported in stage 14

Stage 14 supports `-exec ... ;`, `-exec ... +`, and `-delete`.

`-execdir`, `-ok`, and `-okdir` remain unsupported.
