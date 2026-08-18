## What

## Why

## How I verified

- [ ] `cargo fmt --package synapse-core -- --check`
- [ ] `cargo fmt --package synapse -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] Manual check (describe below if the change is visual)

## Notes

- [ ] User-visible strings are in both Chinese and English
- [ ] `CHANGELOG.md` updated if the change is user-facing
- [ ] `vendor/` was not reformatted
