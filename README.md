# findoxide

`findoxide` is a fresh Rust implementation of Unix `find` that targets GNU `find` syntax while adding a parallel traversal engine.

## v0 scope

- GNU-style argv parsing
- Read-only predicates: `-name`, `-iname`, `-path`, `-ipath`, `-type`, `-true`, `-false`
- Traversal controls: `-mindepth`, `-maxdepth`
- Output actions: `-print`, `-print0`
- Ordered single-worker mode and relaxed-order parallel mode

## Worker selection

`findoxide` keeps the command-line syntax identical to GNU `find`.

Use the `FINDOXIDE_WORKERS` environment variable to control execution mode:

- `FINDOXIDE_WORKERS=1` keeps traversal/output close to GNU ordering
- `FINDOXIDE_WORKERS=4` enables relaxed-order parallel traversal

## Unsupported in read-only v0

The parser accepts side-effecting actions such as `-exec`, `-execdir`, `-ok`, `-okdir`, and `-delete`, but planning rejects them with explicit `unsupported in read-only v0` diagnostics.
