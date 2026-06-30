Got it! Let me clarify a few points before finalizing the plan:

## Clarifications Needed

### 1. WorkerInjector Changes

You said "we shouldn't change WorkerInjector" but I need to understand what you mean:

**Option A**: Don't add D to WorkerInjector's type signature

- Problem: WorkerInjector stores `columns: Arc<[Column<T>]>`, and if Column becomes `Column<T, D>`, then WorkerInjector needs D
- This won't compile unless we keep two versions of Column

**Option B**: Add D to WorkerInjector and add raw_preprocessor field

- `WorkerInjector<T, D>` with `raw_preprocessor: Arc<dyn Fn(&T) -> D>`
- This is what I planned originally

**Option C**: Something else?

I think Option B is necessary for the code to compile. Can you confirm?

### 2. SegmentedInjector Usage

You said "lets keep SegmentedInjector" - I interpret this as:

- Keep the SegmentedInjector type/code (don't delete it)
- But **don't use it** in the new_from_config flow

The new flow would be:

```
String → IndexedInjector → WorkerInjector
```

Not:

```
String → SegmentedInjector → IndexedInjector → WorkerInjector
```

Is this correct? Or do you want to still use SegmentedInjector in new_from_config?

If we keep using SegmentedInjector, then:

- The preprocessor would need to produce the split ranges
- SegmentedInjector would use those ranges to wrap the item
- But this conflicts with having the preprocessor on Worker

Please clarify which approach you want.

---

## Final Plan (Pending Clarification)

Assuming:

- WorkerInjector gets D parameter and raw_preprocessor field (Option B)
- SegmentedInjector is kept but not used in new_from_config

Here's the final plan:

# Final Implementation Plan: Injection Overhaul

## Architecture

**New Flow**: `String` → `IndexedInjector` → `WorkerInjector<Indexed<String>, D>`

**Key Design**:

- Preprocessors are fields on `Worker<T, D>` only
- `WorkerInjector<T, D>` gets `raw_preprocessor` when created from Worker
- `text_preprocessor` stays on Worker, accessed by render layer
- `D = (Result<Text<'static>, String>, Vec<(u32, u32)>)` for config case

---

## Implementation Steps

### Phase 1: Core Type Changes

#### 1.1 Update Column<T> → Column<T, D>

**File**: `matchmaker-lib/src/nucleo/worker.rs`

- Add type parameter: `Column<T, D = ()>`
- Update fn types to take `(&T, &D)`
- Update all methods

#### 1.2 Update Worker<T> → Worker<T, D>

**File**: `matchmaker-lib/src/nucleo/worker.rs`

- Add type parameter: `Worker<T, D = ()>`
- Add fields:
  ```rust
  pub raw_preprocessor: Arc<dyn Fn(&T) -> D + Send + Sync>,
  pub text_preprocessor: Arc<dyn Fn(&T) -> D + Send + Sync>,
  ```
- Update `new()` to take both preprocessors

#### 1.3 Update WorkerInjector<T> → WorkerInjector<T, D>

**File**: `matchmaker-lib/src/nucleo/injector.rs`

- Add type parameter: `WorkerInjector<T, D = ()>`
- Add field: `raw_preprocessor: Arc<dyn Fn(&T) -> D + Send + Sync>`
- Update `push()` to call raw_preprocessor and pass `&D` to column fns
- Update `push_impl<T, D>` to accept `&D`

### Phase 2: Helper Constructors

#### 2.1 Update new_single_column and new_indexable

**File**: `matchmaker-lib/src/nucleo/variants.rs`

- Use `Column<T, ()>` and preprocessor returning `()`
- Column fns accept `&()` and ignore it

### Phase 3: Config Column Builder

#### 3.1 Create build_columns_for_config

**File**: `matchmaker-lib/src/nucleo/variants.rs`

```rust
pub type PreprocessedData = (Result<Text<'static>, String>, Vec<(u32, u32)>);

pub fn build_columns_for_config(
    preprocess: PreprocessOptions,
    split: Split,
    names: Vec<Arc<str>>,
) -> (
    Vec<Column<Indexed<String>, PreprocessedData>>,
    Arc<dyn Fn(&Indexed<String>) -> PreprocessedData + Send + Sync>,
    Arc<dyn Fn(&Indexed<String>) -> PreprocessedData + Send + Sync>,
)
```

- Match on Split variant to create specialized preprocessors
- raw_preprocessor: returns `(Err(String), ranges)`
- text_preprocessor: returns `(Result<Text, String>, ranges)`
- Build columns with format/raw fns that slice using indices from D

### Phase 4: Update new_from_config

#### 4.1 Refactor ConfigMatchmaker::new_from_config

**File**: `matchmaker-lib/src/matchmaker.rs`

- Call `build_columns_for_config` instead of `new_indexable`
- Update type aliases:
  ```rust
  pub type ConfigMMItem = Indexed<String>;
  pub type ConfigInjector = IndexedInjector<
      String,
      WorkerInjector<Indexed<String>, PreprocessedData>
  >;
  ```
- Remove `ConfigMMInnerItem`
- Simplify injector chain: only `IndexedInjector` + `WorkerInjector`
- Remove `OddEnds.splitter` field

### Phase 5: Render Layer

#### 5.1 Update render_row

**File**: `matchmaker-lib/src/ui/results/render.rs`

- Add `text_preprocessor` parameter
- Call it before passing to column format fns

**File**: `matchmaker-lib/src/ui/results/update.rs`

- Pass `worker.text_preprocessor` at call sites

### Phase 6: Type Propagation

#### 6.1 Add D to Matchmaker and related types

**Files**: Multiple

- `Matchmaker<T, S>` → `Matchmaker<T, D, S>`
- Update all files that reference Worker, Column, or Matchmaker
- Selector remains `Selector<T, S>` (no D needed)

### Phase 7: Cleanup

#### 7.1 Remove unused code

**File**: `matchmaker-lib/src/nucleo/injector.rs`

- Remove `AnsiInjector`
- Keep `SegmentedInjector` (for other use cases)
- Keep `IndexedInjector`, `WorkerInjector`

---

## Files Modified

**Core**:

1. `matchmaker-lib/src/nucleo/worker.rs`
2. `matchmaker-lib/src/nucleo/injector.rs`
3. `matchmaker-lib/src/nucleo/variants.rs`
4. `matchmaker-lib/src/matchmaker.rs`
5. `matchmaker-lib/src/ui/results/render.rs`
6. `matchmaker-lib/src/ui/results/update.rs`

**Type Updates**:

- `matchmaker-lib/src/action.rs`
- `matchmaker-lib/src/formatter.rs`
- `matchmaker-lib/src/start.rs`
- `matchmaker-lib/src/ui/mod.rs`
- `matchmaker-cli/src/*.rs`
- Examples

---

**Please confirm the two clarifications above**, then I'll proceed with implementation.
