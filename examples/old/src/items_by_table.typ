// The same data as "items.typ" (the same "/items.json"), but rendered as
// a *separate* one-row table per department instead of one shared table.
//
// This changes how the diff aligns things: each little table is now its
// own top-level element, matched against the others by *content* (the
// same way two headings or two paragraphs are), not by position within
// one shared table -- see "items.typ" for that approach on the identical
// data. Concretely, on this data:
// - Marketing's and Sales's tables are recognized as "the same table,
//   edited" (matched by their unchanged Department cell) and diffed in
//   place, exactly like in items.typ.
// - Support's table has no match anywhere in the new version (nothing
//   else has "Support" in it), and Engineering's has none in the old one,
//   so the two end up compared against each other anyway (they're
//   adjacent once everything else is matched away) and combined into one
//   table showing the clean swap -- also like items.typ, just arrived at
//   by content instead of position.
// - Legal's table is byte-for-byte identical in both versions, so it's
//   recognized as wholly unchanged -- no diff markup at all, same as
//   items.typ's Legal row.
// - HR only exists in the new version: its table has nothing to match
//   against, so the whole table is inserted, underlined.
#let items = json("/items.json")

#table(
  columns: (1fr, 1fr),
  [*Department*], [*Growth*],
)
// A style *set* (`above: 0pt`, not `spacing`, which would also flatten
// the *last* table's trailing gap), not a wrapping #block(...): this
// makes each table below sit flush against the one before it (like rows
// of one bigger table), without bundling them into one Content node.
// `collect()` (in src/diff.rs) traverses a `#set`'s resulting StyledElem,
// so each table still shows up as its own top-level element for the diff
// to match individually -- wrapping them in an actual #block(...) instead
// would turn this whole section into a single opaque `Leaf`, exactly the
// "whole table swapped for another" behavior this file exists to
// contrast with items.typ by NOT doing.
#set block(above: 0pt)
#for item in items [
  #table(
    columns: (1fr, 1fr),
    [#item.name], [#item.growth%],
  )
]
// Reset so whatever follows this #include back in main.typ isn't stuck
// touching the last table too -- a #set here still applies forward past
// the end of this file once it's #include-d, so it needs undoing.
#set block(above: auto)
