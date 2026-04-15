# findoxide

`findoxide` is a fresh Rust implementation of Unix `find` that targets GNU `find` syntax while adding a parallel traversal engine.

## v0 and stage-9 scope

- GNU-style argv parsing
- Global follow-mode options: `-P`, `-H`, `-L`
- Read-only predicates: `-name`, `-iname`, `-path`, `-ipath`, `-type`, `-xtype`, `-true`, `-false`
- Identity/link predicates: `-samefile`, `-inum`, `-links`
- Ownership/account predicates: `-uid`, `-gid`, `-user`, `-group`, `-nouser`, `-nogroup`
- Permission predicate: `-perm`
- Size/time predicates: `-size`, `-empty`, `-used`, `-mtime`, `-atime`, `-ctime`, `-mmin`,
  `-amin`, `-cmin`, `-newer`, `-anewer`, `-cnewer`, full read-only `-newerXY`, `-daystart`
- Time predicates and `-used` accept GNU-style fractional magnitudes such as `0.5`, `+1.25`,
  and `-0.75`
- Symlink-content predicates: `-lname`, `-ilname`
- Traversal controls: `-mindepth`, `-maxdepth`
- Output actions: `-print`, `-print0`
- Ordered single-worker mode and relaxed-order parallel mode
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
- `FINDOXIDE_WORKERS=4` enables relaxed-order parallel traversal

## Follow modes

- `-P` keeps physical traversal semantics and is the default
- `-H` follows symlinks only for command-line roots
- `-L` follows symlinks logically during traversal
- Followed-directory traversal is loop-safe and reports a runtime error instead of recursing forever

## Unsupported in read-only v0

The parser accepts side-effecting actions such as `-exec`, `-execdir`, `-ok`, `-okdir`, and `-delete`, but planning rejects them with explicit `unsupported in read-only v0` diagnostics.
