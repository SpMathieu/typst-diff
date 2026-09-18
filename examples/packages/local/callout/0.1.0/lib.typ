// A single-function demo package, deliberately never published to Typst
// Universe -- only ever meant to be resolved from a local `--package-path`
// under the `local` namespace (`@local/callout:0.1.0`), unlike
// `@preview/cuti` a few directories up, which mirrors a real published
// package.
#let callout(title: "Note", body) = block(
  fill: rgb("#eef6ff"),
  stroke: rgb("#4c8bf5"),
  inset: 8pt,
  radius: 4pt,
  width: 100%,
)[*#title:* #body]
