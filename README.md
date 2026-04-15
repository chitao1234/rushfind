# findoxide

`findoxide` is a fresh Rust implementation of Unix `find` that targets GNU `find` syntax while adding a parallel traversal engine.

## v0 and stage-8 scope

- GNU-style argv parsing
- Global follow-mode options: `-P`, `-H`, `-L`
- Read-only predicates: `-name`, `-iname`, `-path`, `-ipath`, `-type`, `-xtype`, `-true`, `-false`
- Identity/link predicates: `-samefile`, `-inum`, `-links`
- Ownership/account predicates: `-uid`, `-gid`, `-user`, `-group`, `-nouser`, `-nogroup`
- Permission predicate: `-perm`
- Size/time predicates: `-size`, `-mtime`, `-atime`, `-ctime`, `-mmin`, `-amin`, `-cmin`,
  `-newer`, `-anewer`, `-cnewer`, file-reference `-newerXY`, `-daystart`
- Symlink-content predicates: `-lname`, `-ilname`
- Traversal controls: `-mindepth`, `-maxdepth`
- Output actions: `-print`, `-print0`
- Ordered single-worker mode and relaxed-order parallel mode
- Internal performance substrate: lazy entry data access and cheap-first planning for pure read-only `-a` chains
- `-newerXY` currently supports only file-reference forms where both timestamp letters are `a`,
  `c`, or `m`, without `B` or `t`

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
