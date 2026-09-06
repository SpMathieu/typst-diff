// Reads a JSON *array* (as opposed to the single object in "/data.json")
// from a file next to this one, via an absolute path, and renders it as
// ONE table with all the rows -- proof that an #include-d file can itself
// load and render structured data, not just the file that includes it.
//
// Between this version and the other one: values are edited in place
// (Marketing, Sales), one row has nothing in common with the row at the
// same position on the other side (Support/Engineering), one row is
// exactly unchanged (Legal), and one is a trailing addition (HR). See the
// README's "Known limitations" for exactly how the diff aligns and marks
// each of these -- rows are matched by *position*, not by content (see
// "items_by_table.typ", right below this file's #include, for the
// opposite approach and how it behaves differently on the same data).
#let items = json("/items.json")

#table(
  columns: (1fr, 1fr),
  table.header[*Department*][*Growth*],
  ..items.map(item => (item.name, [#item.growth%])).flatten(),
)
