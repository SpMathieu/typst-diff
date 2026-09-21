//! A minimal implementation of `typst::World`.
//!
//! `World` is the interface Typst uses to fetch source files, fonts, etc.
//! This implementation supports two ways of reading the project's `.typ`
//! files and other local assets (JSON data, images...), both letting the
//! main file `#include`/`#import` others by relative or absolute path,
//! resolved exactly like real `typst` does:
//!
//! - [`Backend::Directory`]: from a real directory on the filesystem (the
//!   original, still-default `typst-diff files` mode -- see `main.rs`'s
//!   `FilesArgs`).
//! - [`Backend::Git`]: from a specific revision (tag, branch, or commit)
//!   of a local git repository, reading blobs directly out of git's
//!   object database via `gix` -- no checkout, working directory, or
//!   index involved (`typst-diff git`'s `GitArgs`). This is what makes it
//!   possible to diff, say, `v1.0` against `v2.0` of the same file without
//!   ever having two copies of the repository checked out at once.
//!
//! Fonts are the ones embedded in the compiler, plus (optionally) any
//! found by recursively scanning `--font-path` directories -- see
//! `main.rs`'s `CommonArgs::font_paths` and `FontStore` below. This is
//! always real-filesystem, in both modes: fonts aren't typically committed
//! alongside a Typst project's source.
//!
//! Packages (`#import "@preview/cuti:0.4.0": ...`, `#import
//! "@local/callout:0.1.0": ...`, any namespace) are resolved from a single
//! local directory, structured the same way Typst's own package cache is
//! (`<package-path>/<namespace>/<name>/<version>/...`) -- see `main.rs`'s
//! `CommonArgs::package_path`. No namespace is treated specially: `preview`
//! (packages mirrored from Typst Universe) and `local` (packages you
//! authored yourself and never published anywhere) are resolved exactly
//! the same way, by directory name. Nothing is ever downloaded from the
//! network: only a package that's already present on disk under
//! `--package-path` (e.g. one `typst-cli` itself already downloaded, or
//! one placed there by hand) can be resolved. This, too, is always
//! real-filesystem in both modes, for the same reason as fonts. For a
//! fully-featured `World` (including on-demand downloads from Typst
//! Universe), look at `SystemWorld` in `typst-cli` instead
//! (crates/typst-cli/src/world.rs on the Typst GitHub repo), which is much
//! more complete but also much longer.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use ecow::eco_format;
use typst::diag::{FileError, FileResult, PackageError};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::Font;
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;
use typst_kit::packages::FsPackages;

/// Where a `SimpleWorld`'s project files (as opposed to fonts/packages,
/// which are always read from the real filesystem -- see the module docs
/// above) are read from.
enum Backend {
    /// A real directory on the filesystem (`typst-diff files` mode).
    Directory(PathBuf),
    /// A specific revision of a local git repository (`typst-diff git`
    /// mode): `tree` is the root tree object of that revision (already
    /// resolved once, at construction, from the `--old-rev`/`--new-rev`
    /// the user passed -- see `SimpleWorld::from_git`), and `subdir` is
    /// this project's root *within* that tree, relative to the
    /// repository's own root (mirrors what `Directory`'s `PathBuf` is for
    /// the real-filesystem case, but as a path inside the repo instead of
    /// on disk).
    ///
    /// No live `gix` repository handle is kept around here -- neither
    /// `gix::Repository` nor, perhaps surprisingly, `gix::ThreadSafeRepository`
    /// is actually `Send`/`Sync` in this build (both end up carrying a
    /// `once_cell::unsync::OnceCell` for lazily-resolved remote URL
    /// rewrites, deep in their config cache, regardless of which
    /// `gix` features are enabled) -- which `SimpleWorld` as a whole must
    /// be, per `World: Send + Sync`. So only `repo_path` (to reopen the
    /// repository, cheaply, from scratch) and `tree` (a plain content
    /// hash, `Send`/`Sync`/`Copy` on its own) are kept; see
    /// `Backend::read`.
    Git {
        repo_path: PathBuf,
        tree: gix::ObjectId,
        subdir: PathBuf,
    },
}

impl Backend {
    /// Reads the raw bytes at `vpath` (already resolved against
    /// `--old-root`/`--new-root`, i.e. relative to this backend's own
    /// notion of "project root" -- a real directory, or a subdirectory of
    /// a git tree).
    fn read(&self, vpath: &VirtualPath) -> FileResult<Vec<u8>> {
        match self {
            Backend::Directory(root) => {
                let path = vpath.realize(root).map_err(FileError::Realize)?;
                std::fs::read(&path).map_err(|err| FileError::from_io(err, &path))
            }
            Backend::Git { repo_path, tree, subdir } => {
                // `realize` is pure path arithmetic (join + resolve
                // `.`/`..`), no filesystem access -- it works just as well
                // to compute a path *inside a git tree* as it does a real
                // filesystem path (see `Directory` above), which is why
                // this can reuse it with `subdir` (a repo-relative path)
                // standing in for `root`.
                let path = vpath.realize(subdir).map_err(FileError::Realize)?;
                // Reopened on every call rather than kept as a field (see
                // this variant's doc comment) -- some repeated work
                // (config parsing, etc.), but simple and correct, and
                // still only local filesystem access, no network -- and
                // this isn't a hot path (a handful of files per document).
                let repo = gix::open(repo_path)
                    .map_err(|err| FileError::Other(Some(eco_format!("{err}"))))?;
                read_from_tree(repo, *tree, &path)
            }
        }
    }
}

/// Reads the blob at `path` (relative to the tree `tree_id` names, in
/// `repo`), one path component at a time.
///
/// This exists instead of the obvious one-liner,
/// `repo.find_object(tree_id)?.into_tree().lookup_entry_by_path(path)`,
/// because that doesn't handle `path` crossing into a git submodule: a
/// submodule is recorded in its parent tree as a "commit" entry (a
/// "gitlink"), naming a commit of a *different* repository rather than a
/// tree or blob of this one, and gix's own path lookup doesn't know to
/// treat that specially -- it just tries to look the gitlink's object id
/// up as a tree in `repo`'s own object database, where it never is (a
/// submodule's objects live in its own repository, initialized
/// separately by `git submodule update --init` -- typically into
/// `<repo>/.git/modules/<name>`), and so reports the path "not found"
/// for any file inside a submodule.
///
/// This walks the same path by hand instead so that a gitlink entry
/// encountered along the way (not just as the final component) can be
/// followed into that other repository, opened via
/// [`open_submodule`], and resolved further there against the exact
/// commit this tree records -- not whatever the submodule happens to be
/// checked out at right now. Recurses for a submodule that itself
/// contains submodules.
fn read_from_tree(repo: gix::Repository, tree_id: gix::ObjectId, path: &Path) -> FileResult<Vec<u8>> {
    let not_found = || FileError::NotFound(path.to_path_buf());

    let mut tree = repo
        .find_object(tree_id)
        .map_err(|err| FileError::Other(Some(eco_format!("{err}"))))?
        .into_tree();
    let mut components = path.components().peekable();
    let mut prefix = PathBuf::new();

    while let Some(component) = components.next() {
        let entry = tree
            .find_entry(component.as_os_str().as_encoded_bytes())
            .ok_or_else(not_found)?;
        prefix.push(component);

        if components.peek().is_none() {
            if entry.mode().is_tree() || entry.mode().is_commit() {
                return Err(FileError::IsDirectory);
            }
            let object = entry
                .object()
                .map_err(|err| FileError::Other(Some(eco_format!("{err}"))))?;
            // Can't move `object.data` out: `Object` implements `Drop`
            // (to return its buffer to `repo`'s reuse pool), and Rust
            // forbids partially moving out of a `Drop` type.
            return Ok(object.data.clone());
        }

        if entry.mode().is_commit() {
            let submodule_repo = open_submodule(&repo, &prefix).ok_or_else(|| {
                FileError::Other(Some(eco_format!(
                    "{path:?} is inside the git submodule at {prefix:?}, but it couldn't be \
                     resolved -- is it declared in .gitmodules and initialized \
                     (`git submodule update --init`)?"
                )))
            })?;
            let submodule_tree = submodule_repo
                .find_object(entry.object_id())
                .map_err(|err| FileError::Other(Some(eco_format!("{err}"))))?
                .into_commit()
                .tree()
                .map_err(|err| FileError::Other(Some(eco_format!("{err}"))))?
                .id()
                .detach();
            let rest: PathBuf = components.collect();
            return read_from_tree(submodule_repo, submodule_tree, &rest);
        }

        if !entry.mode().is_tree() {
            return Err(not_found());
        }
        tree = entry
            .object()
            .map_err(|err| FileError::Other(Some(eco_format!("{err}"))))?
            .into_tree();
    }

    Err(not_found())
}

/// Finds, among the git submodules declared in `repo`'s `.gitmodules`,
/// the one mounted at `relative_path` (relative to `repo`'s own root),
/// and opens it as its own repository -- or returns `None` if `repo` has
/// no `.gitmodules`, none of its submodules sit at that path, or the
/// matching one hasn't been initialized yet (no local repository to
/// open).
///
/// Note this reads `.gitmodules` from `repo`'s current worktree/index/
/// `HEAD` (there's no other reasonable choice: submodule checkouts, and
/// thus their location on disk, aren't versioned per-revision the way
/// file contents are) -- exactly what plain `git` itself does. If the
/// revision being diffed added, removed, or moved a submodule since
/// then, this can fail to find it even though the gitlink entry is
/// right there in the tree.
fn open_submodule(repo: &gix::Repository, relative_path: &Path) -> Option<gix::Repository> {
    let submodules = repo.submodules().ok().flatten()?;
    for submodule in submodules {
        let Ok(path) = submodule.path() else { continue };
        if gix::path::from_bstring(path) == relative_path {
            return submodule.open().ok().flatten();
        }
    }
    None
}

/// A `World` backed by either a real project directory or a specific git
/// revision (see [`Backend`]): a main in-memory source file, plus any
/// other local `.typ` file it `#include`s/`#import`s, any other local
/// asset it references (JSON data, images...), and any package it
/// imports, all read on demand.
pub struct SimpleWorld {
    library: LazyHash<Library>,
    /// Shared between the "old" and "new" world (see `main.rs`) -- fonts
    /// don't differ between the two versions being diffed, so there's no
    /// reason to scan `--font-path` directories or re-collect the embedded
    /// fonts twice.
    fonts: Arc<FontStore>,
    source: Source,
    backend: Backend,
    /// Local directory packages (`@preview/cuti:0.4.0`,
    /// `@local/callout:0.1.0`, any namespace...) are resolved from,
    /// structured the way Typst's own package cache is
    /// (`<package_path>/<namespace>/<name>/<version>/...`). `None` means
    /// no package can be resolved at all (the default -- this project
    /// doesn't download packages from the network).
    package_path: Option<PathBuf>,
    /// Other local `.typ` files pulled in via `#include`/`#import`, read
    /// and parsed from disk (or from the git tree) the first time they're
    /// needed, then reused.
    sources: Mutex<HashMap<FileId, Source>>,
}

impl SimpleWorld {
    /// Creates a new world for the `.typ` file at `main_path`, read from a
    /// real directory on the filesystem: `root` is the project root
    /// absolute paths like `/lib/helpers.typ` are resolved against
    /// (relative paths like `./local.typ` are always resolved against the
    /// *importing* file's own directory instead, wherever it is under
    /// `root`).
    ///
    /// `root` doesn't need to be `main_path`'s parent directory: pass
    /// `--old-root`/`--new-root` explicitly (see `main.rs`) when the main
    /// file lives in a subdirectory of the actual project root (e.g.
    /// `<root>/src/main.typ`).
    ///
    /// `fonts` and `package_path` are shared project-wide settings (see
    /// `main.rs`'s `CommonArgs::font_paths`/`CommonArgs::package_path`),
    /// not specific to this one version of the document -- the same
    /// `fonts` is meant to be passed (cheaply, via `Arc::clone`) to both
    /// the "old" and "new" world.
    pub fn from_directory(
        main_path: &Path,
        root: &Path,
        fonts: Arc<FontStore>,
        package_path: Option<PathBuf>,
    ) -> Result<Self> {
        let vpath = VirtualPath::virtualize(root, main_path)
            .with_context(|| format!("{main_path:?} is not inside the project root {root:?}"))?;
        let bytes = std::fs::read(main_path).with_context(|| format!("reading {main_path:?}"))?;
        Self::new(vpath, bytes, Backend::Directory(root.to_path_buf()), fonts, package_path)
    }

    /// Creates a new world for the `.typ` file at `file` (a path relative
    /// to the repository root, the same for every revision), as it reads
    /// at `rev` -- a tag, branch, or commit, resolved the same way `git
    /// rev-parse` resolves one -- in the git repository at `repo_path`.
    /// No checkout happens: file contents are read directly out of git's
    /// object database.
    ///
    /// `root`, if given, is `file`'s project root *within the repository*
    /// (relative to the repository's own root, not to `file`) -- the
    /// `--old-root`/`--new-root` equivalent for this mode, resolving
    /// absolute paths like `/lib/helpers.typ` the same way
    /// [`from_directory`](Self::from_directory)'s `root` does, but against
    /// a subdirectory of the git tree instead of a real filesystem
    /// directory. Defaults to `file`'s own parent directory, exactly like
    /// `from_directory`'s `root` does when `--old-root`/`--new-root` is
    /// omitted. Pass `.` for the repository's own root itself (an empty
    /// path would mean the same thing, but clap rejects an explicit empty
    /// string as "no value" -- there's no real filesystem directory to
    /// point at instead, the way `from_directory`'s callers can point at
    /// their project's top-level directory).
    ///
    /// `fonts` and `package_path` are the same project-wide settings as
    /// `from_directory`'s -- fonts and packages are always read from the
    /// real filesystem, in either mode (see this module's docs).
    pub fn from_git(
        repo_path: &Path,
        rev: &str,
        file: &Path,
        root: Option<&Path>,
        fonts: Arc<FontStore>,
        package_path: Option<PathBuf>,
    ) -> Result<Self> {
        // A throwaway handle, used only to resolve `rev` to its tree once,
        // up front -- `Backend::Git` doesn't keep it (or any other live
        // `gix` repository handle) around, see its doc comment.
        let opened = gix::open(repo_path)
            .with_context(|| format!("opening git repository at {repo_path:?}"))?;
        let commit = opened
            .rev_parse_single(rev)
            .with_context(|| format!("resolving revision {rev:?} in {repo_path:?}"))?
            .object()
            .with_context(|| format!("resolving revision {rev:?} in {repo_path:?}"))?
            .into_commit();
        let tree = commit
            .tree()
            .with_context(|| format!("reading the tree {rev:?} points to in {repo_path:?}"))?
            .id()
            .detach();

        let root = match root {
            // `.` means "the repository root" -- see this method's doc
            // comment for why that's spelled out instead of just using an
            // empty path directly.
            Some(root) if root == Path::new(".") => PathBuf::new(),
            Some(root) => root.to_path_buf(),
            None => file.parent().unwrap_or(Path::new("")).to_path_buf(),
        };
        let vpath = VirtualPath::virtualize(&root, file)
            .with_context(|| format!("{file:?} is not inside the project root {root:?}"))?;
        let backend = Backend::Git { repo_path: repo_path.to_path_buf(), tree, subdir: root.clone() };
        let bytes = backend
            .read(&vpath)
            .with_context(|| format!("reading {file:?} at revision {rev:?} in {repo_path:?}"))?;
        Self::new(vpath, bytes, backend, fonts, package_path)
    }

    /// Shared setup for [`from_directory`](Self::from_directory) and
    /// [`from_git`](Self::from_git): `main_vpath` is the main file's own
    /// virtual path (relative to `backend`'s project root), already read
    /// into `main_bytes` by the caller -- this is what makes absolute
    /// (`/...`) and relative (`./...`) imports resolve correctly from
    /// within it, exactly as they would for any other file.
    fn new(
        main_vpath: VirtualPath,
        main_bytes: Vec<u8>,
        backend: Backend,
        fonts: Arc<FontStore>,
        package_path: Option<PathBuf>,
    ) -> Result<Self> {
        let main_text =
            String::from_utf8(main_bytes).context("main file isn't valid UTF-8")?;
        let file_id = RootedPath::new(VirtualRoot::Project, main_vpath).intern();

        Ok(Self {
            library: LazyHash::new(Library::default()),
            fonts,
            source: Source::new(file_id, main_text),
            backend,
            package_path,
            sources: Mutex::new(HashMap::new()),
        })
    }

    /// Returns the main source file (handy to call `typst_eval::eval`
    /// directly, without going through `World::source`).
    pub fn main_source(&self) -> &Source {
        &self.source
    }

    /// Resolves a file id to its raw bytes: from `self.backend`'s project
    /// root for a project-relative id, from the matching package's
    /// directory under `--package-path` for a package-relative one (e.g.
    /// `@preview/cuti:0.4.0` resolves to
    /// `<package_path>/preview/cuti/0.4.0`, and `@local/callout:0.1.0` to
    /// `<package_path>/local/callout/0.1.0` -- the same layout Typst's own
    /// package cache uses, for any namespace). Packages are always read
    /// from the real filesystem, regardless of `self.backend`.
    fn read(&self, id: FileId) -> FileResult<Vec<u8>> {
        match id.root() {
            VirtualRoot::Project => self.backend.read(id.vpath()),
            VirtualRoot::Package(spec) => {
                let root = self
                    .package_path
                    .as_deref()
                    .and_then(|dir| FsPackages::new(dir).obtain(spec));
                let root =
                    root.ok_or_else(|| FileError::Package(PackageError::NotFound(spec.clone())))?;
                let path = root.resolve(id.vpath())?;
                std::fs::read(&path).map_err(|err| FileError::from_io(err, &path))
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

        let bytes = self.read(id)?;
        let text = String::from_utf8(bytes).map_err(|_| FileError::InvalidUtf8)?;

        let source = Source::new(id, text);
        self.sources.lock().unwrap().insert(id, source.clone());
        Ok(source)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.read(id).map(Bytes::new)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        Some(Datetime::from_ymd(2026, 1, 1).unwrap())
    }
}
