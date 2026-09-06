//! A minimal implementation of `typst::World`.
//!
//! `World` is the interface Typst uses to fetch source files, fonts, etc.
//! This implementation supports real multi-file projects rooted at a
//! single directory: the main `.typ` file can `#include`/`#import` other
//! local `.typ` files, and load other local assets (JSON data, images...),
//! all resolved on the real filesystem relative to that root directory.
//! Fonts are the ones embedded in `typst-assets`.
//!
//! Not supported: packages (`#import "@preview/...": ..."`), since that
//! would need a package downloader/cache — see `PackageStorage` in
//! `typst-kit` if you need to add that. For a fully-featured `World`
//! (packages included), look at `SystemWorld` in `typst-cli` instead
//! (crates/typst-cli/src/world.rs on the Typst GitHub repo), which is much
//! more complete but also much longer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// A `World` backed by a project directory on the real filesystem: a main
/// in-memory source file, plus any other local `.typ` file it
/// `#include`s/`#import`s, and any other local asset it references (JSON
/// data, images...), all read from disk on demand.
pub struct SimpleWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    source: Source,
    /// Real directory the source file lives in. Any other file the
    /// document references by a relative path (`#include`, `#import`,
    /// `json("data.json")`...) is looked up here.
    root: PathBuf,
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
    pub fn new(main_path: &Path, root: &Path) -> Result<Self> {
        let source_text = std::fs::read_to_string(main_path)
            .with_context(|| format!("reading {main_path:?}"))?;

        // Fonts embedded with the compiler (Linux Libertine, etc.).
        // Requires the `fonts` feature on the `typst-assets` crate.
        let mut fonts = Vec::new();
        for bytes in typst_assets::fonts() {
            let bytes = Bytes::new(bytes);
            fonts.extend(Font::iter(bytes));
        }

        let mut book = FontBook::new();
        for font in &fonts {
            book.push(font.info().clone());
        }

        // The main file's own virtual path, derived from its real
        // position relative to `root` — this is what makes absolute
        // (`/...`) and relative (`./...`) imports resolve correctly from
        // within it, exactly as they would for any other file.
        let vpath = VirtualPath::virtualize(root, main_path).with_context(|| {
            format!("{main_path:?} is not inside the project root {root:?}")
        })?;
        let file_id = RootedPath::new(VirtualRoot::Project, vpath).intern();

        Ok(Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            source: Source::new(file_id, source_text),
            root: root.to_path_buf(),
            sources: Mutex::new(HashMap::new()),
        })
    }

    /// Returns the main source file (handy to call `typst_eval::eval`
    /// directly, without going through `World::source`).
    pub fn main_source(&self) -> &Source {
        &self.source
    }
}

impl World for SimpleWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
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

        // Only plain project files are supported (no packages, since
        // there's no package downloader here).
        if !matches!(id.root(), VirtualRoot::Project) {
            return Err(FileError::NotFound(PathBuf::from(id.vpath().get_without_slash())));
        }

        let path = id.vpath().realize(&self.root).map_err(FileError::Realize)?;
        let text = std::fs::read_to_string(&path).map_err(|err| FileError::from_io(err, &path))?;

        let source = Source::new(id, text);
        self.sources.lock().unwrap().insert(id, source.clone());
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        // Only plain project files are supported (no packages, since
        // there's no package downloader here).
        if !matches!(id.root(), VirtualRoot::Project) {
            return Err(FileError::NotFound(PathBuf::from(id.vpath().get_without_slash())));
        }

        let path = id.vpath().realize(&self.root).map_err(FileError::Realize)?;
        std::fs::read(&path)
            .map(Bytes::new)
            .map_err(|err| FileError::from_io(err, &path))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        Some(Datetime::from_ymd(2026, 1, 1).unwrap())
    }
}
