// This project spans several files and directories, laid out like a real
// multi-file Typst project:
//
//   new/
//   ├── data.json            <- the JSON export this report reads
//   ├── items.json           <- a JSON *array*, read by both files below
//   ├── lib/report.typ       <- imported below by an *absolute* path
//   └── src/
//       ├── main.typ          <- this file
//       ├── confidential.typ  <- pulled in below by a *relative* path
//       ├── items.typ         <- pulled in below by a *relative* path
//       └── items_by_table.typ <- pulled in below by a *relative* path
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

// A header (repeated on every page) and a footer with a running page
// count -- both set via #set page(...), not written as ordinary content.
// This exercises a real limitation the diff has to work around:
// `header`/`footer` aren't traversable Content children the way a
// paragraph's text is -- they're *style* properties (`PageElem` in
// typst-library, looked at via a `StyleChain`), so `collect()` (in
// src/diff.rs) can't just walk into them the way it walks into
// SequenceElem/StyledElem. See `root_styles`/`diff_page_marginalia` in
// src/diff.rs for how they're extracted, diffed, and reapplied once for
// the whole document, instead of riding along on every individual atom
// the way an ordinary style (`text(fill: ...)`, `emph()`...) does --
// carrying a *page*-level property per atom is exactly what used to
// blow this up into one page per changed word (see the "Known
// limitations" section of the README for the full story).
//
// The header's title/status/date differ between the two versions
// (diffed word by word, right there in the margin, like any other
// text); the footer's "Page X of Y" is the exact same code in both
// versions, proving a #counter()-driven page number keeps working (and
// isn't itself misdiffed into something broken) once the header/footer
// fix is in place.
#set page(
  header: [
    Quarterly Report -- Final #h(1fr)
    #datetime(year: 2026, month: 2, day: 1).display()
  ],
  footer: context [
    Confidential #h(1fr) Page #counter(page).display() of #counter(page).final().first()
  ],
)

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

// The exact same data again, but this time as one small table *per*
// department instead of one shared table -- see items_by_table.typ for
// how that changes the way the diff aligns and marks the changes.
#include "./items_by_table.typ"

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

#pagebreak()

// This page is identical in both versions -- it only exists so the
// header, footer, and running page count set above keep rendering (and
// keep matching each other) across more than one page.
This second page is here only to show the header, footer, and page
counter above continuing to render correctly across multiple pages.
