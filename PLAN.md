# Modify resultsUI make_table, worker.results, and render_cell to use wrap: bool

## Goals
- Replace the use of `u16::MAX` as a signal for "no wrap" with a explicit `wrap: bool` parameter.
- Wrapping becomes "all or nothing" based on this parameter.
- The `width_limit` parameter will still be used:
    - If `wrap` is `true`, it acts as the wrapping width.
    - If `wrap` is `false`, it acts as a limit for when to stop adding spans to a line (truncation).
- Do not modify the `wrap_text` utility function.

## Changes

### 1. `matchmaker-lib/src/nucleo/worker.rs`

#### `render_cell`
- Update signature to include `wrap: bool`.
- Update logic:
    - Replace `width_limit != u16::MAX` checks with `wrap` or appropriate logic.
    - If `wrap` is `false`, ensure we stop adding graphemes/spans to `current_spans` if it exceeds `width_limit`.
    - Ensure autoscroll logic still uses `width_limit` if appropriate, even when not wrapping (it might need it to know how much context to show).

#### `Worker::results`
- Update signature to include `wrap: bool`.
- Pass `wrap` down to `render_cell`.

### 2. `matchmaker-lib/src/ui/results.rs`

#### `ResultsUI::make_table`
- Update signature to include `wrap: bool`.
- Pass `wrap` to `worker.results`.
- Simplify `width_limits` calculation:
    - Instead of using `u16::MAX` when `!self.config.wrap`, just pass the widths. The `wrap` flag will now handle the behavior.

### 3. `matchmaker-lib/src/ui/mod.rs`

#### `PickerUI::make_table`
- Update the call to `self.results.make_table` to pass `self.config.wrap`.

### 4. Verification
- Verify that `u16::MAX` is no longer used for wrap signaling.
- Ensure all calls to these functions are updated.
- Run tests if available.
