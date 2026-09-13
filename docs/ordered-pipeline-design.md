# Ordered execution pipeline design

This document specifies the replacement for the current ordered single-worker
engine. It covers the streaming redesign, the cancellation model, the
diagnostic ordering rule, and the worker-count policy.

## Problem statement

`run_ordered_pipeline` (`src/ordered/engine.rs`) runs the walk on the calling
thread, hands every entry to evaluator threads over an unbounded channel, and
only starts draining the ready channel **after `walk_ordered` returns**:

```rust
walk_ordered(..., |event| handle_ordered_pipeline_walk_event(...))?;
drop(work_tx);

let mut queue = OrderedReadyQueue::default();
for (sequence, step) in ready_rx { /* dispatch */ }
```

Measured consequences (120k files in 120 directories, warm cache):

| observation | rfd `RUSHFIND_WORKERS=1` | GNU find |
| --- | --- | --- |
| time to first output line | 788 ms | 3.4 ms |
| total wall clock | 855 ms | 85 ms |
| peak RSS, `-print` | 148 MB | 2 MB |
| peak RSS, one 200k-entry directory | 274 MB | 33 MB |

### Measured outcome

After the implementation below, on the same fixtures:

| observation | rfd `RUSHFIND_WORKERS=1` | GNU find |
| --- | --- | --- |
| peak RSS, `-print` (120k files) | 3.2 MB | 2.1 MB |
| wall clock, `-print` (120k files) | 136 ms | 67 ms |
| peak RSS, `-print` (one 200k-entry directory) | 40 MB | 33 MB |
| wall clock, `-print` (one 200k-entry directory) | 296 ms | 1353 ms |

Three defects follow from the same root cause:

1. **No streaming.** Nothing reaches stdout until the traversal has finished,
   so `rfd . -print | head` cannot shortcut and interactive use feels stalled.
2. **Unbounded memory.** Both channels grow with the number of entries, and
   each `EvalStep::PendingAction` carries a cloned `EntryContext`. Peak memory
   is proportional to the tree, not to the work in flight.
3. **Diagnostics interleave wrongly.** The walker writes `WalkEvent::Error`
   diagnostics to stderr immediately, while stdout is written only after the
   walk. With `2>&1`, the diagnostics of the whole run precede the output.

Two further issues live in the same code path:

4. **Thread count is surprising.** `RuntimePolicy::derive` sets
   `evaluation_workers = host_parallelism` whenever the plan is ordered, so
   `RUSHFIND_WORKERS=1` starts 11 threads on a 10-cpu host - and is three times
   slower than `RUSHFIND_WORKERS=4`.
5. **Parallel dispatch is not available for actions.** Plans containing
   commit-sensitive actions (`-exec*`, `-delete`, `-quit`, `-exit`) bypass the
   pipeline entirely through `run_ordered_inline`, because the pipeline has no
   way to observe an action result before the walk is over.

## Target architecture

```
        main thread                evaluator threads              collector thread
   ┌──────────────────┐        ┌────────────────────┐        ┌────────────────────┐
   │ walk_ordered     │ work   │ begin_entry_eval   │ ready  │ OrderedReadyQueue  │
   │  (bounded chan)  ├───────►│ resume_entry_eval  ├───────►│ dispatch actions   │
   │  sequence N      │  cap K │  sequence N        │  cap K │ write stdout/stderr │
   └──────────────────┘        └────────────────────┘        └────────────────────┘
```

- **Everything that can be observed is sequenced.** Entries, directory
  completion frames, and diagnostics all travel through the same
  sequence-numbered stream, so stdout and stderr keep traversal order relative
  to each other. The queue releases item `n` only after `n-1` has been
  dispatched.
- **Both channels are bounded** (capacity `K`, see *Sizing*). The walker blocks
  when `K` entries are unevaluated, evaluators block when `K` results are
  undispatched, and the collector blocks when the queue is empty. In-flight
  memory is therefore `O(K)` regardless of tree size.
- **The collector owns the sink.** `OrderedActionSink` moves to the collector
  thread; actions execute there in sequence order, exactly once, in the same
  order the current ordered engine produces. Action continuations
  (`EvalStep::PendingAction` → `resume_entry_eval`) already model "the result of
  an action feeds the rest of the expression", so `-exec ... \;` short-circuit
  behaviour is preserved.
- **Cancellation is a shared flag.** `OrderedPipelineControl` (an `AtomicBool`, mirroring
  `parallel::control::GlobalControl`)
  is checked by the walker before publishing and by the collector before
  dispatching. When the collector observes a stop request it sets the flag,
  keeps whatever its entry already committed, and drops every later result.

### Single-worker degenerate case

When `evaluation_workers == 1` the pipeline adds two channel hops per entry for
no parallelism. That case uses the existing inline engine instead: the walk,
evaluation, and dispatch all happen on one thread, so output streams with zero
copying and no scheduling overhead. Today `run_ordered_inline` is selected by
`contains_commit_sensitive_action`; after this change it is selected by
`evaluation_workers == 1` and applies to every expression, which is strictly
better than today's routing: today `RUSHFIND_WORKERS=1` with `-print` takes the
buffered pipeline, the slowest of the three configurations.

`contains_commit_sensitive_action` and the `ordered_evaluator_workers`
special case are deleted once the pipeline handles actions.

### Sequence numbering

Sequence numbers are allocated by the walker in emission order, which is the
order GNU find would produce: pre-order for the default traversal, and
children-then-parent for `-depth`. `min_depth` filtering stays in the walker so
filtered entries never consume a sequence number or a channel slot.

### Ordered ready queue

`OrderedReadyQueue` was a `BTreeMap<u64, T>` with one node per in-flight item.
It is now a ring buffer of `K` slots indexed by `sequence % K` plus a `next`
cursor, so insert and pop are `O(1)` with no per-item allocation and
`pop_next` never has to scan. The ring stores `Option<T>`; reaching past the
window is a bug in the pipeline, so the insert path reports an internal error
rather than growing.

### Backpressure comes from permits, not from the channels

Bounding the two channels is not enough on its own. A sequence that stalls
upstream lets the collector keep draining the ready channel into its ring, so
the ring would grow with the tree no matter how small the channels are - which
is how the first implementation ended up reporting "ordered pipeline window
overflow" under load.

The walker therefore takes a permit from a channel pre-filled with `K` before
it publishes anything, and the collector returns one permit per dispatched
item. That makes `published - dispatched <= K` a property of the pipeline
rather than a hope, which is what lets the ring be sized at `K` and nothing
more. When the collector stops, it hands the remaining permits back so a walker
blocked on a full window can observe the stop and unwind.

### `-quit` and `-exit`

Both become normal sequenced actions on the collector thread:

- `-quit` sets the control flag and stops dispatching. Entries already in the
  channels are evaluated but discarded, matching the relaxed engine's
  cancellation contract.
- `-exit N` additionally records the requested status in `RunSummary`.

Because the collector is the only place that observes action outcomes, the walk
is cancelled at the earliest possible point while every side effect committed
before it is still in order. This is why the pipeline no longer needs to fall
back to the inline engine for these actions.

### `-exec ... +` and `-execdir ... +`

`OrderedActionSink` keeps its pending-batch map. Batches fill and flush in
dispatch order, and the final flush happens after the ready channel closes,
before the sink is dropped - the same point in the sequence where GNU find
flushes its last batch.

## Sizing

- `K = (evaluation_workers * 4).clamp(32, 256)` entries.
- Work-channel, ready-channel and release-ring capacities are all `K`, so
  in-flight memory is bounded by the window rather than by the tree.
- A worker that is slow at evaluating leaves the remaining workers busy; the
  walker blocks only when the whole window is full.
- Sampling: `K` is a compile-time constant for the first implementation. If
  profiling shows the walker starving, evaluate batching entries per message
  (chunks of 16-32) rather than enlarging the window.

## Worker-count policy

`RuntimePolicy::derive` currently sets `evaluation_workers = host_parallelism`
in ordered mode. That is surprising and it ignores the requested count. New
rule:

| `RUSHFIND_WORKERS` | engine | threads |
| --- | --- | --- |
| 1 | inline single-threaded | 1 |
| 2..N | streaming pipeline | 1 walker + N evaluators + 1 collector |

`evaluation_workers = requested_workers`, capped by `available_parallelism()`.
The default requested count stays `min(available_parallelism, 4)`.

## Diagnostic ordering

Diagnostics carry sequence numbers like any other item. The collector writes
stdout and stderr from one thread, in sequence order, and flushes stdout before
writing a diagnostic to stderr so that merged (`2>&1`) output matches GNU find's
interleaving. `-ignore_readdir_race` suppression, when implemented, becomes a
predicate on the diagnostic before it is sequenced.

## Compatibility contract

- Output order is unchanged for both engines: identical to GNU find in pre-order
  and in `-depth` post-order.
- Exit status, action failure accounting, and `RunSummary` are unchanged.
- `-exec*`, `-delete`, and `-print*` keep their current commit ordering.
- The relaxed parallel engine is untouched.

## Out of scope

- Per-directory child buffering (`read_children`). GNU find's `fts` also holds
  one directory's entries at a time, and measured RSS on a 200k-entry directory
  is 33 MB for GNU find versus 31 MB for rfd's parallel engine, so this is not a
  divergence. Streaming a directory through an iterator frame remains a
  possible follow-up for the ordered walker only.
- Sorted traversal, `-mtime` boundary parity, and the glob matcher.

## Test plan

- Ordering: differential tests against GNU find for `-print`, `-print0`,
  `-printf`, `-ls`, `-depth`, `-maxdepth`/`-mindepth`, and `-prune`, with
  `RUSHFIND_WORKERS=1` and `2`.
- Streaming: a test that asserts the first line is written before the traversal
  finishes, using a tree large enough that the difference is observable, or a
  synchronization point in a test-only sink.
- Bounded memory: a test that walks a directory tree with more entries than the
  window and asserts peak in-flight count stays within `K` (via a counter in
  the sink rather than RSS).
- Cancellation: `-quit` and `-exit N` stop promptly and return the right status;
  no output appears for entries after the stop.
- Interleaving: with stdout and stderr merged, diagnostics appear between the
  same entries as GNU find's.
- Regression: `cargo test --all-targets` unchanged, and the existing
  differential suite must pass with both worker settings.

## Implementation order

1. Add `OrderedPipelineControl`, the ring-buffer ready queue, and the sequenced
   diagnostic item, with unit tests, without changing behaviour.
2. Route ordered plans with `evaluation_workers == 1` through the inline engine
   (removing the commit-sensitive special case) and verify the differential
   suite.
3. Replace the two unbounded channels with the bounded streaming pipeline and
   delete `run_ordered_pipeline`'s drain-after-walk structure.
4. Switch `RuntimePolicy::derive` to `evaluation_workers = requested_workers`,
   update the README worker table, and measure.
5. Re-measure the table at the top of this document and record the outcome.
