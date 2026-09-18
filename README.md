# typst-diff

Compares two versions of a Typst document (`.typ`) and produces a PDF where:
- **deleted** text appears struck through and in red,
- **added** text appears underlined and in blue,
- a modified sentence appears as "old version struck through" followed by
  "new version underlined" (like Word's track changes).

The diff is computed on the `Content` resolved by Typst (after evaluating
code, functions, variables), not on the raw source text — see `src/diff.rs`
for the details.

Two versions can come from two separate `.typ` files (`typst-diff files`,
the default — see section 3 below), or from the same file at two
different revisions — a tag, a branch, or a commit — of one local git
repository (`typst-diff git`, see "Diffing across git revisions" below).

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
# projects" below for why --old-root/--new-root are needed here, and
# "Fonts and packages" for --font-path/--package-path)
cargo run --release -- files examples/old/src/main.typ examples/new/src/main.typ \
  diff.pdf --old-root examples/old --new-root examples/new \
  --font-path examples/fonts --package-path examples/packages
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
cargo run --release -- files examples/old/src/main.typ examples/new/src/main.typ \
  diff.pdf --old-root examples/old --new-root examples/new \
  --font-path examples/fonts --package-path examples/packages \
  --hide-deletions

# Show added content in standard style (no underline/blue), as if it were
# unchanged text
cargo run --release -- files examples/old/src/main.typ examples/new/src/main.typ \
  diff.pdf --old-root examples/old --new-root examples/new \
  --font-path examples/fonts --package-path examples/packages \
  --hide-additions

# Combine both: a "clean" preview of the new document only
cargo run --release -- files examples/old/src/main.typ examples/new/src/main.typ \
  diff.pdf --old-root examples/old --new-root examples/new \
  --font-path examples/fonts --package-path examples/packages \
  --hide-deletions --hide-additions
```

### Changing the deletion/addition colors

By default, deletions are red and additions are blue. `--deletion-color`
and `--addition-color` override either one — with one of Typst's 18 named
colors (`red`, `orange`, `yellow`, `olive`, `green`, `lime`, `aqua`,
`teal`, `eastern`, `navy`, `blue`, `purple`, `fuchsia`, `maroon`, `black`,
`gray`, `silver`, `white`) or a hex color (`#f30`, `7a03c2`, `abcdefff`):

```bash
cargo run --release -- files examples/old/src/main.typ examples/new/src/main.typ \
  diff.pdf --old-root examples/old --new-root examples/new \
  --font-path examples/fonts --package-path examples/packages \
  --deletion-color orange --addition-color "#00b894"
```

Anything fancier than that (`color.mix(...)`, `oklch(...)`, a named color
outside that list) isn't supported — `parse_color()` in `src/main.rs`
only mirrors Typst's fixed list of named color *constants*
(`Color::RED`, etc.), it doesn't evaluate a real Typst color expression.
Use a hex code for anything not in the list above.

**Quote a hex color that starts with `#`** (`"#f30"`, not `#f30`) — this
isn't specific to `typst-diff`, it's how every shell works: an unquoted
`#` starts a comment, so the shell silently drops it and everything
after it on the line before your command even runs. Without quotes,
`--addition-color #f30` reaches `typst-diff` as `--addition-color` with
no value at all, which is why it fails with "a value is required for
'--addition-color'" rather than a color-parsing error. A hex color
*without* the leading `#` (`f30`, `7a03c2`) doesn't have this problem
and needs no quoting, since it isn't special to the shell.

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

### Fonts and packages

Besides the fonts embedded in the compiler (Libertinus Serif, New
Computer Modern...), `--font-path` adds a directory to recursively
search for more, the same way `typst compile --font-path` does. Pass it
multiple times to search multiple directories; it applies to both
versions of the document, since fonts aren't something that differs
between them.

`--package-path` resolves `#import "@preview/...": ...` and
`#import "@local/...": ...`-style package imports from a local
directory, structured the same way Typst's own package cache is:
`<package-path>/<namespace>/<name>/<version>/...` (so, for instance,
`@preview/cuti:0.4.0` resolves to `<package-path>/preview/cuti/0.4.0`,
and `@local/callout:0.1.0` to `<package-path>/local/callout/0.1.0`).
**No package is ever downloaded from the network** — this only reads
what's already on disk, whichever namespace it's under (`preview`,
`local`, or any other name — `world.rs` doesn't treat any namespace
specially). To populate that directory:
- for a `preview` package, either copy it over from wherever `typst
  compile` itself already cached it (`~/.cache/typst/packages` on
  Linux, `~/Library/Caches/typst/packages` on macOS,
  `%LOCALAPPDATA%\typst\packages` on Windows — see
  `--package-cache-path` in `typst compile --help`), or download and
  unpack its `.tar.gz` from
  `https://packages.typst.org/preview/<name>-<version>.tar.gz` by hand;
- for a `local` package — one you wrote yourself and never published
  anywhere — just place its files directly at
  `<package-path>/local/<name>/<version>/`, the same way real `typst`'s
  own package data directory works for unpublished packages.

Both example projects (`examples/old/`, `examples/new/`) use this in
their "Status" section, near the end of `src/main.typ`: a font found
only by scanning `examples/fonts` (Tahoma — see
`examples/fonts/README.md` for why the actual font file isn't checked
into this repository), a package from the `preview` namespace
(`@preview/cuti`, mirrored from
[Typst Universe](https://typst.app/universe/package/cuti/) under its own
MIT license), and a package from the `local` namespace (`@local/callout`,
a trivial one-function package written just for this example, under this
project's own Apache-2.0 license, to demonstrate the `local` namespace
specifically) — see `examples/packages/`. That's why every command in
this README passes `--font-path examples/fonts --package-path
examples/packages`: without them, evaluating `examples/old/src/main.typ`
or `examples/new/src/main.typ` fails outright on the unresolved
`@preview/cuti`/`@local/callout` imports (both are scoped to that one
`#block[...]`, so nothing *else* in the document is affected — but the
imports still have to resolve for evaluation to succeed at all).

### Diffing across git revisions

Everything above uses `typst-diff files`, which reads the old and new
version from two separate `.typ` files (optionally full separate project
directories, as `examples/old`/`examples/new` are). `typst-diff git`
diffs the *same* file instead, as it reads at two different revisions —
a tag, a branch, or a raw commit, resolved the same way `git rev-parse`
resolves one — of one local git repository, without ever checking either
revision out (no `git checkout`, no second working copy):

```bash
cargo run --release -- git examples/git-project report.typ diff.pdf \
  --old-rev v1.0 --new-rev v2.0

# A branch or a raw commit work exactly the same way as a tag
cargo run --release -- git examples/git-project report.typ diff.pdf \
  --old-rev v2.0 --new-rev develop
```

`examples/git-project` is its own separate git repository (added here as
a **submodule**, so `git clone`/`git status` won't show its files as part
of this one) with a small `report.typ` committed a few times: tagged
`v1.0` and `v2.0`, with a further work-in-progress `develop` branch on
top of `v2.0`, to exercise all three kinds of revision. If it's empty
after cloning this repository, fetch it once with:
```bash
git submodule update --init examples/git-project
```

`--old-root`/`--new-root` work the same way as in `files` mode (see
"Multi-file projects" above), except relative to the *repository's* own
root instead of a real filesystem directory — there's no checkout to
point at, so a path like `/lib/helpers.typ` is instead looked up directly
in the resolved revision's git tree, at
`<repository>/<old-root-or-new-root>/lib/helpers.typ`. They default to
`FILE`'s own parent directory, exactly like `files` mode's do.
`--font-path`, `--package-path`, and every other flag (`--hide-deletions`,
`--deletion-color`...) work identically in both modes — fonts and
packages are always read from the real filesystem, never from the
repository, in either mode.

Internally, `git` mode reads blobs directly out of git's object database
via the pure-Rust [`gix`](https://docs.rs/gix) crate (see `Backend::Git`
in `world.rs`) — no shelling out to `git`, no libgit2/OpenSSL, and,
crucially, nothing ever written to disk or to the repository's working
tree, so it's safe to run against a repository you have other work in
progress in.

`examples/output/` has the resulting PDFs checked in, so you can see what
both modes produce without building anything: `files-mode.pdf` (from the
first command in section 3) and `git-mode.pdf` (from
`examples/git-project`'s `example-old`/`example-new` branches, which
mirror `examples/old`/`examples/new` exactly) are byte-for-byte
identical — the whole point of `git` mode being just another way to feed
the same two versions in.

## 4. Project structure

```
typst-diff/
├── Cargo.toml          # dependencies
├── README.md           # this file
├── examples/
│   ├── old/             # example: old version of a small project
│   │   ├── data.json     # a single JSON object, loaded via an absolute path
│   │   ├── items.json    # a JSON *array*, read by both files below
│   │   ├── lib/
│   │   │   └── report.typ  # imported via an absolute path
│   │   └── src/
│   │       ├── main.typ            # entry point compared by typst-diff
│   │       ├── confidential.typ    # included via a relative path
│   │       ├── items.typ           # included via a relative path
│   │       └── items_by_table.typ  # included via a relative path
│   ├── new/             # example: new version of the same project
│   │   └── ...           # same layout, updated content throughout
│   ├── fonts/            # --font-path example dir (empty except a README)
│   ├── packages/         # --package-path example dir (preview + local)
│   ├── git-project/      # `typst-diff git` example (its own repo -- a
│   │   └── ...            # submodule; see its own note below)
│   └── output/           # checked-in PDFs produced by the commands above
│       ├── files-mode.pdf
│       └── git-mode.pdf   # byte-identical to files-mode.pdf
└── src/
    ├── main.rs          # entry point: CLI (files/git subcommands), orchestration
    ├── world.rs         # minimal implementation of `typst::World`
    └── diff.rs           # Content flattening + diff + reconstruction
```

## 5. Known limitations (possible improvements)

- **Local multi-file projects and local packages work, network package
  downloads don't**: `SimpleWorld` (in `world.rs`) reads any file the
  project references — `#include "other.typ"`, `#import
  "/lib/helpers.typ": foo`, `json("/data.json")`, `image("logo.png")`...
  — from the real filesystem, both absolute (`/...`) and relative
  (`./...`) paths, resolved exactly like real `typst` does (see
  "Multi-file projects" above for the resolution rules and the
  `--old-root`/`--new-root` flags). Packages (`#import
  "@preview/...": ..."`, `#import "@local/...": ..."`, any namespace)
  resolve too, but only from a local directory passed via
  `--package-path` (see "Fonts and packages" above) — this project has
  no package downloader, so a package has to already be on disk there.
  For on-demand downloads from Typst Universe (what real
  `typst compile` does when a package isn't found locally), see
  `UniversePackages`/`SystemPackages::new` in `typst-kit` (the
  `system-downloader` feature) if you need to add that — it wasn't
  pulled in here to keep this project's dependency tree (and any
  network-related build/deployment concerns, e.g. cross-compiling to
  musl) minimal. For a fully-featured `World` (downloads included), look
  at `typst-cli`'s `SystemWorld` instead (see the Typst GitHub repo,
  `crates/typst-cli/src/world.rs`).
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

  Positional matching is specific to rows *within* one table, though —
  `examples/*/src/items_by_table.typ` renders the exact same data as
  `items.typ`, but as a separate one-row table per department instead of
  one shared table, and gets the *content*-based matching every other
  top-level element gets (the same mechanism a heading or a paragraph is
  matched by): each small table is aligned against the others by what's
  in it, not by where it sits. On this particular data the two approaches
  happen to reach the same visual result (compare the two tables the
  example produces), but they get there differently, and could easily
  diverge on data where positions and content-identity disagree — e.g. a
  reordered list would confuse the positional version but not the
  content-matched one, while the content-matched version could
  misidentify an edit as an unrelated delete-then-insert if `atom_key`
  can't find enough left in common (see `try_recurse`'s "shares a word"
  guard) where the positional version, having nothing better to compare
  a row against, would still pair rows up and diff them in place.
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
- **Page header/footer are diffed once for the whole document, not per
  word — and only when they're set once, near the top**: `#set
  page(header: ..., footer: ...)` (and any other page-construction
  property: `margin`, `numbering`, `paper`...) doesn't produce ordinary
  Content the way a paragraph does — it's a *style* property (`PageElem`
  in typst-library), invisible to `collect()`'s usual
  flatten-into-comparable-atoms traversal in `src/diff.rs`. Early on, this
  project just let it ride along like any other style (`text(fill:
  ...)`, `emph()`...), carried on every individual atom for
  `atom_to_content`/`wrap_deleted`/`wrap_added` to reapply when
  reconstructing the annotated output. That silently dropped header/
  footer changes from the diff entirely (whichever the *last*-styled atom
  happened to carry is what showed, on every page) — and, worse, since a
  page property really did differ between the struck-through old atoms
  and the underlined new ones, Typst inserted an automatic page break at
  every single boundary where the two disagreed, fragmenting the whole
  document into roughly one page per changed word. `collect()` now
  strips `PageElem` properties out of what it carries per atom; `main.rs`
  diffs the header/footer content separately (`root_styles`/
  `diff_page_marginalia` in `src/diff.rs`) and reapplies the result once,
  on top of the whole document, alongside the *new* document's other page
  properties. `root_styles` finds this by walking the same nested
  `SequenceElem`/`StyledElem` structure `collect()` does, in document
  order — which reliably finds one `#set page(...)` wherever it sits
  among a document's top-level content (this doesn't have to be the very
  first statement — see `examples/*/src/main.typ`, where it comes after
  some `#import`/`#let` lines), but doesn't attempt to make sense of
  *several* independent `#set page(header: ...)` calls further down the
  same document, each meant to apply to only part of it — an edge case
  outside what this project's examples exercise.
- **A `context [...]` expression's *body* can't be told apart from
  another one's**: `atom_key()` falls back to a `Leaf` atom's `Debug`
  representation to compare it across versions (see its doc comment), but
  a `context [...]` block (what `#counter(page).display()` and similar
  expressions expand to) is a `ContextElem` wrapping a `Func` closure, and
  `Func`'s own `Debug` impl only ever prints `Func(..)` for an anonymous
  closure — never what's inside it. Two *different* `context [...]`
  bodies (say, one showing `Page X` and another showing something else
  entirely) are therefore indistinguishable to the diff, and register as
  "the same atom, unchanged" (see `examples/*/src/main.typ`'s footer: its
  `context [... #counter(page).display() ...]` is intentionally identical
  code in both versions, which is the only case this can be relied on to
  render sensibly). What still works well is the common case demonstrated
  there: a `#counter()`/`context` expression is normally surrounded by
  ordinary text (`"Page "`, `" of "`...), and *that* text diffs correctly
  word by word — it's only a change to what the counter/context
  expression itself computes that goes unnoticed.
- **Layout now stabilizes across multiple passes — evaluation still
  doesn't**: Typst normally re-runs layout several times so
  introspection-dependent content (a table of contents,
  `#counter(page).final()`, "as seen on page N" cross-references...) can settle once the
  document's page count and every element's final location are known —
  `layout()` in `src/main.rs` now mirrors the `typst` crate's own
  `compile_impl` stabilization loop for this (re-laying out the same
  `Content` with each pass's own `Introspector` fed into the next, up to
  `MAX_ITERS` times, via `comemo::Constraint` to detect when a pass no
  longer depends on anything that changed since the last one) — this is
  what makes `examples/*/src/main.typ`'s `counter(page).final()` (used in
  "Page X of Y") resolve correctly. `eval_to_content`, however, still
  only evaluates once, with no introspector at all — fine for the vast
  majority of documents, whose *content* (as opposed to its later layout)
  doesn't depend on page counts or element positions, but not a full
  port of `compile_impl`'s loop (which re-evaluates together with
  re-laying-out) for the rare document that needs it.
- **`git` mode reopens the repository on every file it reads, and can't
  see uncommitted changes**: `Backend::Git` (in `world.rs`) deliberately
  never keeps a live `gix::Repository`/`gix::ThreadSafeRepository` around
  as a field — as of `gix` 0.87, neither is actually `Send`/`Sync`
  regardless of which crate features are enabled (both carry a lazily
  resolved remote-URL-rewrite cache, deep inside their config, that isn't
  either), which `SimpleWorld` as a whole must be. So only the repository
  *path* and the already-resolved revision's tree *hash* are kept (both
  plain, `Send`/`Sync` values on their own), and the repository is
  reopened — config parsed again, etc. — for every single file read. Fine
  for a document with a handful of files (the common case), needlessly
  slow for a project with hundreds of them; if that turns out to matter,
  the fix is to eagerly walk the whole resolved tree once (`Tree::traverse`
  in `gix`) into an in-memory `HashMap<PathBuf, Vec<u8>>` at construction
  time instead of reading on demand, sidestepping the `Send`/`Sync` issue
  entirely by not keeping any `gix` type around at all afterward.
  Separately, since revisions are resolved from git's object database, not
  a checkout, there's no way to diff against a working tree's uncommitted
  changes — both `--old-rev` and `--new-rev` have to be something already
  committed (`git rev-parse` accepts more exotic things too, like
  `@{upstream}` or a merge-base expression, which work here as well, but
  not the working tree itself). And `FILE` is assumed to be the same path
  in both revisions — there's no support for diffing across a rename.

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

## License

Licensed under the [Apache License, Version 2.0](LICENSE) — the same
license Typst itself uses.
