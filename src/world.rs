//! A minimal implementation of `typst::World`.
//!
//! `World` is the interface Typst uses to fetch source files, fonts, etc.
//! This implementation supports real multi-file projects rooted at a
//! single directory: the main `.typ` file can `#include`/`#import` other
//! local `.typ` files, and load other local assets (JSON data, images...),
//! all resolved on the real filesystem relative to that root directory.
//!
//! Fonts are the ones embedded in the compiler, plus (optionally) any
//! found by recursively scanning `--font-path` directories -- see
//! `main.rs`'s `Args::font_paths` and `FontStore` below.
//!
//! Packages (`#import "@preview/cuti:0.4.0": ...`, `#import
//! "@local/callout:0.1.0": ...`, any namespace) are resolved from a single
//! local directory, structured the same way Typst's own package cache is
//! (`<package-path>/<namespace>/<name>/<version>/...`) -- see `main.rs`'s
//! `Args::package_path`. No namespace is treated specially: `preview`
//! (packages mirrored from Typst Universe) and `local` (packages you
//! authored yourself and never published anywhere) are resolved exactly
//! the same way, by directory name. Nothing is ever downloaded from the
//! network: only a package that's already present on disk under
//! `--package-path` (e.g. one `typst-cli` itself already downloaded, or
//! one placed there by hand) can be resolved. For a fully-featured `World`
//! (including on-demand downloads from Typst Universe), look at
//! `SystemWorld` in `typst-cli` instead (crates/typst-cli/src/world.rs on
//! the Typst GitHub repo), which is much more complete but also much
//! longer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::Font;
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;
use typst_kit::packages::FsPackages;

/// A `World` backed by a project directory on the real filesystem: a main
/// in-memory source file, plus any other local `.typ` file it
/// `#include`s/`#import`s, any other local asset it references (JSON
/// data, images...), and any package it imports, all read from disk on
/// demand.
pub struct SimpleWorld {
    library: LazyHash<Library>,
    /// Shared between the "old" and "new" world (see `main.rs`) -- fonts
    /// don't differ between the two versions being diffed, so there's no
    /// reason to scan `--font-path` directories or re-collect the embedded
    /// fonts twice.
    fonts: Arc<FontStore>,
    source: Source,
    /// Real directory the source file lives in. Any other file the
    /// document references by a relative path (`#include`, `#import`,
    /// `json("data.json")`...) is looked up here.
    root: PathBuf,
    /// Local directory packages (`@preview/cuti:0.4.0`,
    /// `@local/callout:0.1.0`, any namespace...) are resolved from,
    /// structured the way Typst's own package cache is
    /// (`<package_path>/<namespace>/<name>/<version>/...`). `None` means
    /// no package can be resolved at all (the default -- this project
    /// doesn't download packages from the network).
    package_path: Option<PathBuf>,
    /// Other local `.typ` files pulled in via `#include`/`#import`, read
    /// and parsed from disk the first time they're needed, then reused.
    sources: Mutex<HashMap<FileId, Source>>,
}

impl SimpleWorld {
    /// Creates a new world for the `.typ` file at `main_path`, whose
    /// project root is `root` (the directory absolute paths like
    /// `/lib/helpers.typ` are resolved against; relative paths like
    /// `./local.typ` are always resolved against the *importing* file's
    /// own directory instead, wherever it is under `root`).
    ///
    /// `root` doesn't need to be `main_path`'s parent directory: pass
    /// `--old-root`/`--new-root` explicitly (see `main.rs`) when the main
    /// file lives in a subdirectory of the actual project root (e.g.
    /// `<root>/src/main.typ`).
    ///
    /// `fonts` and `package_path` are shared project-wide settings (see
    /// `main.rs`'s `Args::font_paths`/`Args::package_path`), not specific
    /// to this one version of the document -- the same `fonts` is meant to
    /// be passed (cheaply, via `Arc::clone`) to both the "old" and "new"
    /// world.
    pub fn new(
        main_path: &Path,
        root: &Path,
        fonts: Arc<FontStore>,
        package_path: Option<PathBuf>,
    ) -> Result<Self> {
        let source_text =
            std::fs::read_to_string(main_path).with_context(|| format!("reading {main_path:?}"))?;

        // The main file's own virtual path, derived from its real
        // position relative to `root` — this is what makes absolute
        // (`/...`) and relative (`./...`) imports resolve correctly from
        // within it, exactly as they would for any other file.
        let vpath = VirtualPath::virtualize(root, main_path)
            .with_context(|| format!("{main_path:?} is not inside the project root {root:?}"))?;
        let file_id = RootedPath::new(VirtualRoot::Project, vpath).intern();

        Ok(Self {
            library: LazyHash::new(Library::default()),
            fonts,
            source: Source::new(file_id, source_text),
            root: root.to_path_buf(),
            package_path,
            sources: Mutex::new(HashMap::new()),
        })
    }

    /// Returns the main source file (handy to call `typst_eval::eval`
    /// directly, without going through `World::source`).
    pub fn main_source(&self) -> &Source {
        &self.source
    }

    /// Resolves a file id to its real filesystem path: inside the project
    /// root (`self.root`) for a project-relative id, inside the matching
    /// package's directory under `--package-path` for a package-relative
    /// one (e.g. `@preview/cuti:0.4.0` resolves to
    /// `<package_path>/preview/cuti/0.4.0`, and `@local/callout:0.1.0` to
    /// `<package_path>/local/callout/0.1.0` -- the same layout Typst's own
    /// package cache uses, for any namespace).
    fn realize(&self, id: FileId) -> FileResult<PathBuf> {
        match id.root() {
            VirtualRoot::Project => id.vpath().realize(&self.root).map_err(FileError::Realize),
            VirtualRoot::Package(spec) => {
                let root = self
                    .package_path
                    .as_deref()
                    .and_then(|dir| FsPackages::new(dir).obtain(spec));
                let root =
                    root.ok_or_else(|| FileError::Package(PackageError::NotFound(spec.clone())))?;
                root.resolve(id.vpath())
            }
        }
    }
}

impl World for SimpleWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<typst::text::FontBook> {
        self.fonts.book()
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            return Ok(self.source.clone());
        }

        if let Some(source) = self.sources.lock().unwrap().get(&id) {
            return Ok(source.clone());
        }

        let path = self.realize(id)?;
        let text = std::fs::read_to_string(&path).map_err(|err| FileError::from_io(err, &path))?;

        let source = Source::new(id, text);
        self.sources.lock().unwrap().insert(id, source.clone());
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let path = self.realize(id)?;
        std::fs::read(&path)
            .map(Bytes::new)
            .map_err(|err| FileError::from_io(err, &path))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        Some(Datetime::from_ymd(2026, 1, 1).unwrap())
    }
}
