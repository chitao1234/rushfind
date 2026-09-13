# BSD `-s` sorted traversal design

BSD `find` implementations on macOS, FreeBSD, and NetBSD support `-s`, which
sorts entries within each directory before traversal. This is a traversal
policy, not a global output sort.

## Semantics

For each directory, direct children are ordered lexicographically by their
pathname component. The directory itself remains governed by the selected
preorder or postorder mode:

- default preorder: emit the directory, then sorted child subtrees;
- `-d`/`-depth`: emit sorted child subtrees, then the directory completion;
- `-s` does not reorder command-line roots globally;
- `-s` sorts independently inside every root and every descended directory;
- `-s -prune` sorts the siblings that are actually visited, while prune still
  prevents descent into the selected subtree;
- `-s -maxdepth` and `-s -mindepth` retain their existing depth semantics.

The initial implementation uses the project’s byte-preserving displayed path
representation for comparison. Because all children of one directory share
the same parent prefix, sorting complete child paths is equivalent to sorting
their direct names and avoids a second basename implementation. This gives
stable, deterministic ordering for non-UTF-8 Unix names and keeps Windows path
normalization inside the existing platform path module if the option is later
accepted there.

## Planning and execution

Add `sort_children: bool` to `CompatibilityOptions` and `TraversalOptions`.
Recognize `-s` as a leading compatibility option before path/expression
splitting. Keep `TraversalOrder::{PreOrder,DepthFirstPostOrder}` unchanged.

Any plan with `sort_children` uses the ordered execution engine. Relaxed
parallel traversal cannot promise BSD sibling ordering, and forcing ordered
mode avoids accidentally emitting a sorted traversal in an unspecified order.
The ordered evaluator may continue to use its bounded internal evaluator pool;
the sequence collector already preserves traversal sequence and action order.

The option itself does not add a runtime expression atom, so it cannot be
reordered by the optimizer and does not affect predicate short-circuiting.

## Walker implementation

The ordered walker already materializes one directory’s children before pushing
them onto its stack. After `read_children` succeeds, sort the child vector when
`options.sort_children` is true, then keep the existing reverse-push logic. In
postorder mode this produces sorted sibling subtrees while preserving the
directory completion frame.

The parallel worker path should apply the same sort before chunk accumulation
as a defensive invariant, although normal planning routes `-s` to ordered mode.
Keeping the sort at the walker boundary also makes backend-provided child
streams deterministic for tests.

Use a cached sort key per child (`sort_by_cached_key`) based on
`platform::path::display_bytes(&child.path)`. Do not use lossy `String`
conversion, locale collation, or a global final-output sort.

## Diagnostics and compatibility

`-s` takes no operand. A missing or misplaced option remains an ordinary
unsupported expression token according to the existing parser rules. The help
text should list `-s` with the BSD traversal options.

`-s` is accepted on every platform because the semantics are well-defined by
the shared path-byte representation. If a future platform requires a different
collation contract, that should be introduced only after a demonstrated
semantic conflict.

## Tests

- Parser recognizes leading `-s` and records it in compatibility options.
- Preorder output is sorted within each directory but roots retain argument
  order.
- Nested directories prove sorting is local, not a global path sort.
- `-d -s` emits sorted descendants before each parent.
- `-s -prune` does not descend into pruned directories.
- `-s -maxdepth` and `-s -mindepth` preserve depth filtering.
- Raw non-UTF-8 names sort by bytes without lossy conversion.
- `RUSHFIND_WORKERS>1` still selects ordered execution for `-s` and emits the
  same sequence as one worker.
- macOS differential coverage compares `rfd -s` with `/usr/bin/find -s`.

## Implementation order

1. Add parser/planner state and force ordered execution.
2. Sort ordered walker children per directory.
3. Apply the defensive sort in parallel worker child collection.
4. Add CLI, planner, and macOS differential tests.

Priority 1 ACL work remains deferred.

