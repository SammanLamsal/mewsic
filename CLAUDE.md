# CLAUDE.md

mewsic is a minimal MPD frontend in Rust (egui/eframe, tokio, mpd_client).

## Priorities

Write every change with **cleanliness, readability, and maintainability** in mind. These come before brevity and cleverness. Someone new to the codebase should be able to read a function once and understand it.

## Readability

- Prefer plain, explicit code over clever Rust. Ten obvious lines beat one dense line.
- Avoid long combinator chains, macro tricks, and trait gymnastics when a `match`, `if let`, or `for` loop says the same thing more clearly.
- Use descriptive names for functions, variables, and types. Avoid abbreviations unless they are universal (`uri`, `id`, `ui`).
- Keep functions small and focused on one job. Extract a helper when a block needs a comment to explain what it does.
- Return early to keep nesting shallow.

## Comments

- Keep comments few. Code should explain *what* it does on its own.
- Comment only the non-obvious *why*: a workaround, an MPD quirk, a deliberate trade-off.
- Don't narrate code, leave commented-out code, or add comments that restate a name.
- Short `///` doc comments on public types are fine when they add context the name can't.

## Maintainability

- Match the surrounding code: its structure, naming, idioms, and module layout.
- Put code where it belongs: MPD communication in `mpd.rs`, persistence in `queues.rs`, widgets under `src/ui/`. UI widgets push `Action`s; `App::apply` carries them out.
- Don't duplicate logic. Reuse existing helpers before writing new ones.
- Remove dead code, unused imports, and leftover debugging output.
- Keep changes scoped to the task. Don't refactor unrelated code in the same change.
- Handle errors with `anyhow` and `?`. Don't `unwrap()` on anything that can fail at runtime.
- Stay cross-platform (Linux X11/Wayland, macOS, Windows). Use platform-neutral APIs such as `directories` instead of hardcoded Unix paths.

## Before finishing

```sh
cargo fmt
cargo clippy
cargo test
```

Fix any warnings these produce in the code you touched.
