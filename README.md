# typst-diff

Compares two versions of a Typst document (`.typ`) and produces a PDF where:
- **deleted** text appears struck through and in red,
- **added** text appears underlined and in blue,
- changed words are annotated inside the retained content; unchanged words remain normal.

The diff is computed on the `Content` resolved by Typst (after evaluating
code, functions, variables), not on the raw source text — see `src/diff.rs`
for the details.

Two versions can come from two separate `.typ` files (`typst-diff files`,
the default — see section 3 below), or from the same file at two
different revisions — a tag, a branch, or a commit — of one local git
repository (`typst-diff git`, see "Diffing across git revisions" below).
`typst-diff --gui` launches a graphical form for either instead of
building the command by hand — see "Graphical interface" below.

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
git submodule update --init --recursive examples/git-project
```
(`--recursive` because of `v3.0`/`submodule-template`, below — a plain
`--init` is enough if all you need is `v1.0`/`v2.0`/`develop`.)

`v3.0` (branch `submodule-template`, also on top of `v2.0`) additionally
declares its own **nested** git submodule, `template/` (pointing at
[`typst-diff-example-template`](https://github.com/SpMathieu/typst-diff-example-template)),
and has `report.typ` `#import` a function from it:
```bash
cargo run --release -- git examples/git-project report.typ diff.pdf \
  --old-rev v2.0 --new-rev v3.0
```
This exercises `git` mode reading a file that lives *inside* a
submodule — a gitlink entry in the tree, naming a commit of another
repository, rather than a tree or blob of this one (see
`read_from_tree`/`open_submodule` in `world.rs`) — which it wasn't
always able to do. One caveat this has, that plain `files` mode
doesn't: `open_submodule` locates a submodule's own repository via
`.gitmodules`, read from `examples/git-project`'s *current*
worktree/index/`HEAD` — exactly what plain `git` itself does, since
where a submodule actually lives on disk isn't versioned per-revision
the way file contents are. So diffing a revision that (like `v3.0`)
declares a submodule the currently-checked-out revision doesn't (like
`v2.0`, or `v1.0`/`develop`) requires checking out one that does first
— `git -C examples/git-project checkout v3.0` — even though nothing
else about `git` mode needs a checkout at all.

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

### Graphical interface

```bash
cargo run --release --locked -- --gui
```

The resizable window has a **Files / Git revisions** form on the left and
an in-memory document preview on the right. **Browse...** selects real
filesystem paths. In Git mode, **Choose...** lists files/directories from
Git trees and branches/tags from the selected repository; typed revisions
can also be commit IDs. Git `FILE` and root fields refer to the repository,
not the GUI's working directory.

**Preview** renders the diff without writing a PDF. **Auto preview** refreshes
it after a short pause in form edits. Scroll or drag to pan, use Ctrl+scroll
or pinch to zoom, and **Fit width** to fit the page horizontally. These view
controls do not change the document produced by the CLI. External edits to
files, JSON data, fonts, packages or moving Git refs are not watched: click
**Preview** again after changing them.

**Generate** saves the annotated PDF at **Output PDF**, using the same
arguments and diff/layout functions as the preview. **Open** opens the
exported PDF. Rendering/export run on worker threads so the form stays
responsive.

#### Copying the equivalent CLI command

**Copy command**, immediately next to **Generate**, copies a complete command
to the system clipboard. It includes the running executable, `files` or
`git`, the input/output paths, both Git revisions when applicable, explicit
roots, all font paths, the package path, both hide switches when enabled,
and both colors (including transparency). An empty Output PDF uses `diff.pdf`.
Copying does not generate a file or execute the command.

The button is enabled after a successful preview whose settings still match
the form. It is disabled while rendering, after an error, or when document
settings change before the preview is refreshed. This avoids copying a
command for different content from the preview currently displayed. Changing
only **Output PDF** does not require another preview; the command uses the
new destination. Clipboard feedback is independent of the PDF export status.

Real filesystem paths and the executable are made absolute, so the command
can be pasted from another working directory on the same machine. Git tree
paths (`FILE`, `--old-root`, `--new-root`) stay repository-relative. Optional
empty fields remain omitted, preserving the CLI defaults. Arguments are
quoted, including spaces, apostrophes and hex colors; options use
`--name=value`, and `--` separates them from positional paths.

On Linux/macOS the command targets **sh, bash or zsh**. Under WSL, paste it
into the same WSL environment, not a Windows terminal session. A native
Windows build copies a **PowerShell 7.3+** command, not a `cmd.exe` command;
a local script block selects standard native argument passing without
changing the caller's preference. Paths with invalid Unicode, NUL or line
breaks are rejected rather than silently altered.

Reproduction assumes the same files/JSON, Git refs, packages, fonts, binary
and relevant environment. The clipboard command is not an archived snapshot,
and branch names are not pinned to commits. Preview rasterization and PDF
viewer antialiasing can differ; PDF bytes/metadata are not promised identical.

#### Linux desktop requirements

On Debian/Ubuntu, including Ubuntu-on-WSL, install `libegl1` when the window
reports "Found no glutin configs matching the template", and
`libgtk-3-dev pkg-config` to build the native file pickers:

```bash
sudo apt install libegl1 libgtk-3-dev pkg-config
```

See "Known limitations" for the existing WSLg/display troubleshooting notes.
The GUI uses `eframe` (window/widgets and clipboard), `rfd` (file dialogs),
`open` (PDF viewer), and `typst-render` (preview rasterization). These are
already dependencies; clipboard export adds none. Only the GUI needs a
working desktop/display. CLI operation and non-rendering unit tests do not.

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
│   │   ├── ...            # submodule; see "Diffing across git revisions")
│   │   └── template/      # nested submodule, only at v3.0/submodule-template
│   └── output/           # checked-in PDFs produced by the commands above
│       ├── files-mode.pdf
│       └── git-mode.pdf   # byte-identical to files-mode.pdf
└── src/
    ├── main.rs                        # CLI arguments, orchestration, shared rendering
    ├── gui.rs                         # form, preview, PDF export and copy-button wiring
    ├── gui/
    │   ├── command.rs                 # CLI serialization, quoting and clipboard feedback
    │   └── command/
    │       └── tests.rs               # CLI round-trip and preview-state tests
    ├── world.rs                       # filesystem / Git implementation of typst::World
    ├── diff.rs                        # matching and recursive-diff entry points
    └── diff/
        ├── scoped.rs
        ├── tables.rs
        └── footnotes.rs
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
- **Styles and document structure follow the new version; style-only edits
  are not highlighted**: `src/diff/scoped.rs` keeps the new document's
  `StyledElem`/`SequenceElem` scopes and inserts annotations into that tree.
  Page, paragraph and alignment styles are not reapplied to each word.
  Deleted text inherits the style at its insertion point. Matching ignores
  many style properties, so a color/font-only edit may be rendered without
  a change marker. Changing an element's kind can still be a replacement.
- **Recursive diffing is explicit, not universal**: recognized headings,
  `strong`, `emph`, links and supported containers retain their new-version
  properties while their bodies are compared. The table and footnote modules,
  when present below, have dedicated handling. Unknown elements remain
  opaque, and structurally incompatible replacements may still be shown as
  a whole deletion followed by an insertion. The matcher does not generally
  track arbitrary moved content across the document.
- **JSON-generated tables are compared cell by cell when their organization
  stays compatible**: the diff operates on evaluated Typst content, not JSON
  source lines. `src/diff/tables.rs` preserves the new table/cell properties
  and diffs the cell bodies, including supported header/footer cells,
  horizontal/vertical rules, inherited columns and unchanged cell spans.
  Supported `figure`, `block`, `align`, `box` and `pad` wrappers are retained.
  Rows are not associated by a JSON record ID. Insertions, removals, reordering
  or changed spans/layout can require the older positional fallback or a
  whole-table replacement. A changed number should stay within its cell;
  the patch is not a general table-structure or row-movement tracker.
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
- **Page headers/footers are preserved from the new version, not annotated
  as a separate diff**: scoped reconstruction keeps local page settings in
  place, including a later section that disables its header or footer.
  The old global `root_styles` / `diff_page_marginalia` merge is no longer
  applied by the patched main rendering path. Retained deletions can still
  change pagination naturally. Content hidden in deferred contexts remains
  subject to the context limitation below.
- **Footnote edits keep one note**: `src/diff/footnotes.rs` diffs a matched
  note's body while retaining the new note's structure. A modified or added
  note has an addition-colored number; only an entirely deleted note has a
  deletion-colored number. Partial deletion within a note is a modification.
  Numbers are colored, not struck through or underlined; removed body text
  is struck through and added text is underlined, including at the bottom
  of the page. Unchanged text remains normal. `--hide-deletions` removes
  deletions; `--hide-additions` removes addition highlighting without removing
  new text. Deleted notes kept for review still count in diff numbering.
  Custom `footnote.entry` rules that ignore the note's body/numbering, global
  reference coloring, ambiguous note moves and deferred contexts have limits.
- **Deferred `context` content is not generally diffed internally**: some
  evaluated elements wrap functions whose debug representation does not
  identify their captured content. Preserving the new tree preserves its
  contexts, but does not make edits hidden inside them detectable. This
  affects templates or generated tables/notes that are only constructed
  during later contextual realization. An unchanged-looking diff is not
  evidence of no change inside such a context.
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
- **`--gui` needs a working display, a few system packages, and
  (depending on your desktop) a nudge away from `eframe`/`rfd`'s
  defaults**: unlike `files`/`git` mode, `--gui` (`gui.rs`) needs a real
  X11/Wayland/macOS/Windows display to open a window on — it won't work
  over a plain SSH session or in most containers/CI. On WSLg (Ubuntu on
  WSL2) specifically, four real issues were hit and fixed while building
  this, in order — see the `eframe`/`rfd` dependencies' comment in
  `Cargo.toml` for the full account:
  1. `eframe`'s default `wgpu` renderer failed outright ("Failed to
     create surface for any enabled backend") — fixed by switching to
     the `glow` (OpenGL) renderer (`gui::run()`'s `NativeOptions`,
     `eframe`'s `glow` feature).
  2. `glow` then failed too ("Found no glutin configs matching the
     template") because the `libegl1` system package wasn't installed —
     `sudo apt install libegl1` fixed it. Not something `Cargo.toml` can
     install for you.
  3. With both of those fixed, the window opened but the process then
     crashed a moment later ("Io error: Connection reset by peer", "winit
     EventLoopError") — `accesskit` (screen-reader accessibility support,
     on by `eframe`'s default features) was trying to reach a D-Bus/AT-SPI
     service that isn't there (or isn't happy) under WSLg. Dropped from
     `eframe`'s enabled features for now (along with `wgpu` and `links`,
     neither of which this form uses either).
  4. With the window finally open and stable, every **Browse…** button
     silently did nothing — `rfd`'s default "xdg-portal" backend asks
     the `xdg-desktop-portal` D-Bus service to show a dialog, and WSLg
     doesn't run one (no full desktop session — `echo
     $XDG_CURRENT_DESKTOP` is empty). Switched to `rfd`'s `gtk3` feature
     instead, which shows GTK's own file chooser directly, with no portal
     service involved — needs `libgtk-3-dev` (`+ pkg-config`) to build,
     `libgtk-3-0` to run.

  A sandboxed dev container used earlier in development never got any
  renderer working at all, not even after installing `libegl1` — most
  likely missing GLX/EGL support entirely in its particular X11
  forwarding setup, software rendering included, rather than anything
  `gui.rs` can detect or route around on its own. If `--gui` still won't
  open a window (or its **Browse…** buttons still don't do anything) for
  you after all the above, that's the kind of environment limitation to
  suspect next. `files`/`git` mode (and `gui.rs`'s own non-rendering
  logic, covered by `cargo test`) don't depend on any of this and work
  regardless.

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
