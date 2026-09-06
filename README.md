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

# Build and run on the two example projects provided (see "Multi-file
# projects" below for why --old-root/--new-root are needed here)
cargo run --release -- examples/old/src/main.typ examples/new/src/main.typ \
  diff.pdf --old-root examples/old --new-root examples/new
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
cargo run --release -- examples/old/src/main.typ examples/new/src/main.typ \
  diff.pdf --old-root examples/old --new-root examples/new --hide-deletions

# Show added content in standard style (no underline/blue), as if it were
# unchanged text
cargo run --release -- examples/old/src/main.typ examples/new/src/main.typ \
  diff.pdf --old-root examples/old --new-root examples/new --hide-additions

# Combine both: a "clean" preview of the new document only
cargo run --release -- examples/old/src/main.typ examples/new/src/main.typ \
  diff.pdf --old-root examples/old --new-root examples/new \
  --hide-deletions --hide-additions
```

### Multi-file projects

A real Typst document is rarely a single file: it typically `#include`s or
`#import`s other local `.typ` files, and loads data/images by path. Both
example projects are laid out this way to demonstrate it:

```
examples/old/            (examples/new/ mirrors this exactly)
├── data.json             # loaded from src/main.typ via an absolute path
├── lib/
│   └── report.typ        # a `summary()` function, imported absolutely
└── src/
    ├── main.typ           # the file you actually pass to typst-diff
    └── confidential.typ   # pulled in via a *relative* path
```

Typst resolves paths in a `.typ` file one of two ways:
- **`/leading/slash`** — an absolute path, resolved against the **project
  root** (wherever `--root` points, in real `typst`; here,
  `--old-root`/`--new-root`), regardless of which file it's written in.
- **`./relative`** or **`bare/relative`** — resolved against the
  **directory of the file that contains it**, regardless of where the
  project root is.

Since `examples/old/src/main.typ` sits one level below its project root
(`examples/old/`), its `#import "/lib/report.typ": summary` and
`json("/data.json")` calls need that root to be told explicitly —
otherwise they'd (wrongly) resolve against `src/` instead, where those
files don't exist. Its `#include "./confidential.typ"`, on the other hand,
works regardless of the root, since it's resolved against `src/` either
way. Run without `--old-root`/`--new-root` to see the resulting "file not
found" error for yourself.

If your own project's entry point already sits at your project's root,
you can just omit `--old-root`/`--new-root` — they default to the
respective input file's own directory, exactly like `typst compile`
without `--root` does.

## 4. Project structure

```
typst-diff/
├── Cargo.toml          # dependencies
├── README.md           # this file
├── examples/
│   ├── old/             # example: old version of a small project
│   │   ├── data.json     # a single JSON object, loaded via an absolute path
│   │   ├── items.json    # a JSON *array*, loaded from items.typ below
│   │   ├── lib/
│   │   │   └── report.typ  # imported via an absolute path
│   │   └── src/
│   │       ├── main.typ         # entry point compared by typst-diff
│   │       ├── confidential.typ # included via a relative path
│   │       └── items.typ        # included via a relative path
│   └── new/             # example: new version of the same project
│       └── ...           # same layout, updated content throughout
└── src/
    ├── main.rs          # entry point: CLI, orchestration
    ├── world.rs         # minimal implementation of `typst::World`
    └── diff.rs           # Content flattening + diff + reconstruction
```

## 5. Known limitations (possible improvements)

- **Local multi-file projects work, packages don't**: `SimpleWorld` (in
  `world.rs`) reads any file the project references — `#include
  "other.typ"`, `#import "/lib/helpers.typ": foo`, `json("/data.json")`,
  `image("logo.png")`... — from the real filesystem, both absolute
  (`/...`) and relative (`./...`) paths, resolved exactly like real
  `typst` does (see "Multi-file projects" above for the resolution rules
  and the `--old-root`/`--new-root` flags). It does **not** support
  packages (`#import "@preview/...": ..."`), since that needs a package
  downloader/cache with network access — see `PackageStorage` in
  `typst-kit` if you need to add that. For a fully-featured `World`
  (packages included), look at `typst-cli`'s `SystemWorld` instead (see
  the Typst GitHub repo, `crates/typst-cli/src/world.rs`).
- **Style follows the new document, but pure style changes aren't
  flagged**: `collect()` (in `src/diff.rs`) carries each atom's styles
  (color, weight, italics...) along as it flattens a `StyledElem`, and
  `diff_content()` always reapplies the *new* document's styles to
  unchanged text — so `text(fill: ..., weight: ..., style: ...)[...]`
  content renders correctly even when the diff has nothing to say about
  it (see `examples/*/src/confidential.typ`'s `Confidential` line).
  What this doesn't do is *flag* a pure style change as a change: since
  `atom_key()` deliberately ignores styles when matching atoms between
  versions (so text isn't wrongly treated as deleted+re-added just
  because its color changed), a paragraph that only turned bold looks
  identical to the diff — it's shown correctly styled, just without a
  strikethrough/underline marker anywhere. Separately, a style change
  made via an element `collect()` doesn't traverse (e.g. wrapping text in
  `strong()` in one version but not the other, as `status` in the
  examples does) is a different case entirely: `strong(...)` and plain
  text become different *kinds* of atoms (a `Leaf` vs `Word`s), so they
  never match, and the diff shows a full delete of the old, plain version
  followed by a full insert of the new, bold one.
- **Word-by-word diff inside headings, `strong()`, `emph()`, and links,
  but not arbitrary elements**: by default, `collect()` treats an element
  it doesn't specifically know how to traverse (an image, a table, a
  citation...) as one opaque block — editing anything inside it strikes
  through/re-underlines the *whole* thing. Headings (`= Title`),
  `strong()`, `emph()`, and links are the exceptions: `diff_content()`'s
  `recurse_into_replaced()` specifically recognizes when the top-level
  diff wholesale-replaced one of these with another of the *same* kind,
  and in that case diffs their bodies word by word instead, rewrapping the
  result in a fresh element that keeps the new document's own other
  attributes (heading level/numbering, link destination...). See
  `examples/*/src/main.typ` for a live demonstration of all four (the
  heading title, plus a paragraph with a partly-edited bold phrase,
  italic phrase, and link). These can't just be traversed and flattened
  away during `collect()` the way `SequenceElem`/`StyledElem` are, since
  unlike those, their wrapper is what makes them render as a
  heading/bold/italic/a link at all — it has to be rebuilt around the
  diffed body, not discarded. Add a case to `impl_body_element!`'s list in
  `src/diff.rs` for other single-body wrapper elements you want the same
  treatment for.

  This recursion only kicks in when the two bodies share at least one
  word — otherwise (e.g. one client's name entirely replaced by an
  unrelated one, both set in `strong()`) it's skipped in favor of the
  plain whole-span delete-then-insert, since word-level diffing of two
  completely unrelated phrases can scramble them across each other in a
  confusing order (spaces "match" regardless of the words around them, so
  even totally different text can appear to partially align). The
  "Dupont Inc." → "Martin & Co." client name in the examples exercises
  this fallback deliberately, right next to the cases that do recurse.
- **Tables are diffed row by row, matched by *position* — not by
  content**: unlike the `BodyElement` wrapper elements (one `body` each),
  a table is *many* bodies (one per cell) that need to be grouped into
  rows first, so it gets its own function, `recurse_into_table()`: row 1
  of the old table is compared against row 1 of the new one, row 2
  against row 2, and so on — deliberately a plain positional comparison,
  not a search for which old row "best matches" which new row by content
  (an earlier version tried that with a content-based Myers diff over
  rows; it broke down as soon as *every* row's value changed at once,
  with no unchanged row left to anchor the alignment, falling back to
  stacking the whole old table on top of the whole new one instead of
  showing in-place edits). Each pair of rows at the same position is then
  diffed cell by cell (each cell recursed into via `diff_content`, so an
  edited value stays a word-level diff inside that one cell), *unless*
  the two rows share no whole cell at all, in which case the old row is
  struck through and the new one inserted right after it rather than
  scrambling two unrelated rows together — see `row_shares_content()`
  (whole-cell matching, not word-level: two unrelated percentages sharing
  the literal "%" doesn't count as the row having something in common).
  Extra rows past the shorter table's length are likewise whole-row
  deletions/insertions. `examples/*/items.json` exercises all of this at
  once: values edited in place, a row with nothing in common with its
  counterpart, a truly identical row, and a trailing insertion.

  This only handles the common case, though: `recurse_into_table()` gives
  up (falling back to the plain whole-table swap) if the two tables were
  given a different `columns` layout, or either uses
  `table.hline`/`table.vline` (manually placed lines have no well-defined
  place to end up once rows shift around them) — see `table_cells()`'s
  doc comment. It also always keeps the *new* table's
  `table.header`/`table.footer` as-is rather than diffing them. And since
  rows are matched purely by position, a row inserted or removed anywhere
  but the end shifts the pairing for every row after it (there's no
  attempt to detect that rows moved) — the "shares a word"/"shares a
  cell" guards only stop unrelated *paired* rows from being scrambled
  together, they don't recover the "really" corresponding rows once
  positions have shifted.
- **A structural change (e.g. text → table) is an unrelated
  delete-then-insert, not a "reformat"**: the diff has no concept of "this
  paragraph became a table with the same information" — a run of text and
  a table share no comparable atoms at all (see `atom_key`), so the whole
  old paragraph is struck through and the whole new table is inserted
  right after it, same as any other two completely unrelated pieces of
  content replacing each other. This is arguably the *correct* behavior
  (there's no meaningful word-level correspondence to show), just worth
  knowing about — see the "Regional highlights" paragraph/table pair at
  the end of `examples/*/src/main.typ` for what this looks like in
  practice.
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
