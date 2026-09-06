// This project spans several files and directories, laid out like a real
// multi-file Typst project:
//
//   new/
//   ├── data.json           <- the JSON export this report reads
//   ├── lib/report.typ      <- imported below by an *absolute* path
//   └── src/
//       ├── main.typ         <- this file
//       └── confidential.typ <- pulled in below by a *relative* path
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

// Relative include: "./confidential.typ" is resolved against this file's
// own directory (src/), wherever the project root is.
#include "./confidential.typ"
