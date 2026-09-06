# typst-diff

Compares two versions of a Typst document (`.typ`) and produces a PDF where:
- **deleted** text appears struck through and in red,
- **added** text appears underlined and in blue,
- a modified sentence appears as "old version struck through" followed by
  "new version underlined" (like Word's track changes).

The diff is computed on the `Content` resolved by Typst (after evaluating
code, functions, variables), not on the raw source text — see `src/diff.rs`
for the details.

## ⚠️ Know this before you start

This project relies on **internal** crates of the Typst compiler
(`typst-eval`, `typst-layout`...) which are not designed as a stable public
API: they change from one version to the next. **You may well need to
adjust a few code details after the first build**, especially if you use a
Typst version different from the one tested here. The README explains how
to get unstuck in the "If `cargo build` fails" section below.

## 1. Install Rust

You need a **recent** version of Rust (the Typst compiler uses modern
language features). The Rust version shipped by `apt` on Ubuntu/Debian is
almost always too old — use `rustup`, the official installer, instead:

- **macOS / Linux**: open a terminal and run:
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
  then restart your terminal (or run `source "$HOME/.cargo/env"`).

- **Windows**: download and run `rustup-init.exe` from
  <https://rustup.rs>, then follow the instructions (pick the default
  option).

Then check that everything is installed correctly:
```bash
rustc --version
cargo --version
```

## 2. Open the project

A Rust project is just a folder with a `Cargo.toml` file at its root (here,
`typst-diff/`). You can open it with any editor:

- **VS Code** (recommended for beginners):
  1. Install the **rust-analyzer** extension (Marketplace → search for
     "rust-analyzer").
  2. `File > Open Folder...` → select the `typst-diff/` folder.
  3. rust-analyzer will index the project (download dependencies, analyze
     the code) — the first time, this takes one or two minutes and uses
     some bandwidth (Typst has a lot of dependencies).
  4. Compile errors show up directly underlined in red in the editor, as in
     any Rust project.

- **Terminal only** (no IDE): just `cd` into the folder and use the `cargo`
  commands below.

- **RustRover / CLion / another JetBrains IDE**: `Open` → select the
  folder, the Rust plugin (often already included) automatically picks up
  the `Cargo.toml`.

## 3. Build and run

From the `typst-diff/` folder:

```bash
# Check that it builds (fast, doesn't produce an optimized binary)
cargo check

# Build and run on the two example files provided
cargo run --release -- examples/old.typ examples/new.typ diff.pdf
```

The `diff.pdf` file is created in the current folder. Open it to see the
result: deleted words struck through in red, added words underlined in
blue.

`--release` is recommended: Typst runs noticeably slower in debug mode (but
*builds* faster in debug — to iterate on errors, `cargo check` without
`--release` is enough and is faster).

### Showing/hiding deletions and additions

Two flags let you control what the annotated PDF shows:

```bash
# Hide deleted content entirely (no strikethrough text at all)
cargo run --release -- examples/old.typ examples/new.typ diff.pdf --hide-deletions

# Show added content in standard style (no underline/blue), as if it were
# unchanged text
cargo run --release -- examples/old.typ examples/new.typ diff.pdf --hide-additions

# Combine both: a "clean" preview of the new document only
cargo run --release -- examples/old.typ examples/new.typ diff.pdf --hide-deletions --hide-additions
```

## 4. Project structure

```
typst-diff/
├── Cargo.toml          # dependencies
├── README.md           # this file
├── examples/
│   ├── old.typ          # example: old version
│   └── new.typ          # example: new version
└── src/
    ├── main.rs          # entry point: CLI, orchestration
    ├── world.rs         # minimal implementation of `typst::World`
    └── diff.rs           # Content flattening + diff + reconstruction
```

## 5. Known limitations (possible improvements)

- **Single file only**: `SimpleWorld` (in `world.rs`) doesn't support
  `#include`, images, or packages (`#import "@preview/..."`). For a real
  multi-file project, you'd need a more complete `World` implementation,
  along the lines of `typst-cli`'s `SystemWorld` (see the Typst GitHub
  repo, `crates/typst-cli/src/world.rs`).
- **Word-by-word diff only within raw text**: pure style changes (e.g. a
  word becoming bold without changing its text) are not detected, and
  content inside elements like `strong()`/`emph()`/links is treated as a
  single block rather than being diffed word by word internally. See the
  comments in `src/diff.rs`, function `collect()`, for where to extend this
  behavior.
- **Single layout pass**: Typst normally re-runs layout several times to
  stabilize cross-references (table of contents, counters...). This
  project only does a single pass — plenty to try out the idea, but worth
  revisiting for complex documents with lots of cross-references.

## If `cargo build` fails

This is expected if Typst's internal API has changed since this project
was written. In order:

1. Note the actual Typst version that got downloaded (visible in
   `Cargo.lock`, look for `name = "typst"`).
2. Check the generated docs for that exact version:
   ```bash
   cargo doc --open -p typst-library -p typst-eval -p typst-layout
   ```
   This opens, in your browser, the exact documentation of the types the
   code uses (`World`, `Engine`, `Library`...), with the right signatures
   for YOUR version.
3. Compare with the source code of the `typst` crate itself on GitHub, at
   the tag matching your version:
   `https://github.com/typst/typst/blob/v<VERSION>/crates/typst/src/lib.rs`
   — the `compile_impl` function there is the reference for everything
   `src/main.rs` does (evaluating, then laying out, a `World`'s content).
4. Adjust field/parameter names accordingly — the project's logic (flatten
   → diff → rebuild) won't change, only the "plumbing" for accessing
   Typst's internal structures is likely to move.
