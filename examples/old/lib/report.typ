// Shared report-building logic, imported by "src/main.typ" via an
// *absolute* path ("/lib/report.typ") -- resolved against the project
// root, not against the importing file's own directory.
#let summary(data) = [
  This quarter, sales grew by #data.growth percent compared to the
  previous quarter.

  The main client remains *#data.client*.
]
