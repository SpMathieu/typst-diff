// Reads a JSON *array* (as opposed to the single object in "/data.json")
// from a file next to this one, via an absolute path, and renders it as a
// table -- proof that an #include-d file can itself load and render
// structured data, not just the file that includes it.
//
// Between this version and the other one, "Support" is dropped and two
// departments ("Engineering", "Legal") are added -- see the README's
// "Known limitations" for how the diff handles a changed table: it isn't
// diffed row by row, so the whole table is replaced wholesale rather than
// just the rows that actually changed.
#let items = json("/items.json")

#table(
  columns: (1fr, 1fr),
  table.header[Department][Growth],
  ..items.map(item => (item.name, [#item.growth%])).flatten(),
)
