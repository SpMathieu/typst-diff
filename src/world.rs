//! A minimal implementation of `typst::World`.
//!
//! `World` is the interface Typst uses to fetch source files, fonts, etc.
//! Here we keep it as simple as possible: a single in-memory source file
//! (no `#include`, no external images), with the fonts embedded in
//! `typst-assets`.
//!
//! If you need to support projects with multiple files / images, look at
//! `SystemWorld` in `typst-cli` instead
//! (crates/typst-cli/src/world.rs on the Typst GitHub repo), which is much
//! more complete but also much longer.

use std::path::PathBuf;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// An in-memory `World`, for compiling a single string.
pub struct SimpleWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
    source: Source,
}

impl SimpleWorld {
    /// Creates a new world from the source text of a `.typ` file.
    pub fn new(source_text: String) -> Self {
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

        let vpath = VirtualPath::new("main.typ").expect("\"main.typ\" is a valid virtual path");
        let file_id = RootedPath::new(VirtualRoot::Project, vpath).intern();

        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
            source: Source::new(file_id, source_text),
        }
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
            Ok(self.source.clone())
        } else {
            Err(FileError::NotFound(PathBuf::from(id.vpath().get_without_slash())))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        Err(FileError::NotFound(PathBuf::from(id.vpath().get_without_slash())))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        Some(Datetime::from_ymd(2026, 1, 1).unwrap())
    }
}
