// This project spans several files and directories, laid out like a real
// multi-file Typst project:
//
//   new/
//   ├── data.json           <- the JSON export this report reads
//   ├── items.json          <- a JSON *array*, read by items.typ below
//   ├── lib/report.typ      <- imported below by an *absolute* path
//   └── src/
//       ├── main.typ         <- this file
//       ├── confidential.typ <- pulled in below by a *relative* path
//       └── items.typ        <- pulled in below by a *relative* path
//
// Since this file lives one level down from the project root (in src/),
// run typst-diff with `--old-root`/`--new-root` pointing at "examples/old"
// and "examples/new" respectively -- otherwise the absolute imports below
// would (wrongly) be resolved against "src/" instead. See the README's
// "Multi-file projects" section.

// Absolute import: "/lib/report.typ" is resolved against the project
// root, not against this file's own directory (src/).
#import "/lib/report.typ": summary

// Same rule applies to any path-taking function, like `json()`.
#let data = json("/data.json")

= Quarter Report

#summary(data)

Revenue: #data.revenue

// Proof that editing *inside* strong()/emph()/a link only marks the
// changed word(s), not the whole span -- see `recurse_into_replaced` in
// src/diff.rs. Compare this paragraph's diff to the heading above: same
// idea, extended from headings to these three element kinds.
Our *core numbers* remain accurate, and our methodology stays
_highly transparent_. Full details are available on
#link("https://example.com/reports/q1-2026-v2")[this quarter's portal].

// Relative include: "./confidential.typ" is resolved against this file's
// own directory (src/), wherever the project root is.
#include "./confidential.typ"

// A department-by-department breakdown, loaded and rendered by
// items.typ (its own include, its own JSON file).
#include "./items.typ"

// Proof of a *structural* change: this quarter's regional breakdown was
// plain text in the other version, rewritten as a table here -- see the
// README's "Known limitations" for how the diff handles two versions of
// the same information in unrelated shapes (it can't tell they're "the
// same content reformatted": text and a table share no comparable atoms,
// so the whole paragraph is struck through and the whole table is
// inserted, with nothing in between).
#table(
  columns: (auto, auto),
  table.header[Region][Growth],
  [North], [5%],
  [South], [8%],
  [East], [3%],
)
