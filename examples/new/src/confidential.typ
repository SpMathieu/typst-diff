// Pulled into "main.typ" via a *relative* path ("./confidential.typ") --
// resolved against this file's own directory (src/), regardless of where
// the project root is.

// Same wording, different formatting: plain text in the other version,
// `strong()` (bold) here. This is a *pure* style change with no text
// change -- see the "Known limitations" section of the README for how
// the diff tool handles this case.
#let status = "On track"
Status: *#status*

#text(fill: red, weight: "bold", style: "italic")[Confidential — internal use only, do not distribute]
