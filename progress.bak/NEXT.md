Current: belt.rs (TODO)
Next step: adapt /tmp/create_x/mixins/create/belt.rs → modules/belt.rs
Blocker: 15 DONE_UNVERIFIED modules not wired in android_bundled.rs (not modules!)
Helpers: 58 helpers from mixin.toml — NONE found as separate files (2 vanilla files incorrectly matched)
Wiring: 8 rows for android_bundled declarations, dispatch, hooks, reset, final build test
Modules: 41 total (15 exist, 26 missing) — run progress.sh for per-kind counts
Verify: cargo check -p horizon-client 2>&1 | tail -20
Priority: wire 15 existing modules in android_bundled.rs before creating new ones