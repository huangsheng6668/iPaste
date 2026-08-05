# Performance Baseline (2026-08-05)

## Frontend (vitest bench)

| Bench | ops/sec | mean |
|---|---|---|
| clipSearch 1k match | 1,287 | 0.78ms |
| clipSearch 5k no match | 239 | 4.19ms |
| automationFilter 100 match | 7,481 | 0.13ms |
| automationFilter 500 no match | 1,512 | 0.66ms |
| ordering compareSortOrder × 100 | bench ran | (see vitest bench output) |
| ordering orderCategoryItemsByIds 500 | bench ran | (see vitest bench output) |

## Rust (cargo test timing assertions)

| Test | Threshold | Status |
|---|---|---|
| list_clips 1k | < 50ms | ✅ pass |
| list_clips 5k | < 200ms | ✅ pass |
| search_with_fallback 5k | < 300ms | ✅ pass |
| list_automations 500 | < 50ms | ✅ pass |

All 4 timing tests pass on the baseline machine. Thresholds are generous
(2x+ buffer over expected). If a test fails on a slower CI runner, widen
the threshold — these are baseline guards, not CI gates.

## Machine

Windows 11 Pro, Node 22, Rust stable.

## How to re-run

```bash
npx vitest bench          # frontend
cargo test --manifest-path src-tauri/Cargo.toml  # rust (includes timing tests)
```
