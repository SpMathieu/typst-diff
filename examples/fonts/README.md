# `--font-path` example directory

Empty on purpose: `typst-diff`'s Apache-2.0 license covers this repository's
own code and the packages vendored under `../packages/` (`cuti` is MIT,
`callout` is this project's own), but not arbitrary third-party fonts, so
none are committed here.

To render the "Status" section of `examples/old/src/main.typ` /
`examples/new/src/main.typ` in Tahoma the way it's styled, copy Tahoma
into this directory yourself from wherever it's already installed on
your own, licensed copy of Windows/Office -- e.g. from WSL:

```bash
cp /mnt/c/Windows/Fonts/tahoma.ttf /mnt/c/Windows/Fonts/tahomabd.ttf examples/fonts/
```

Without it, `--font-path examples/fonts` still works (it's just an empty
directory to scan), and Typst silently falls back to its default font
instead of erroring.
