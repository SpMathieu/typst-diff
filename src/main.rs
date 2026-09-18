mod diff;
mod gui;
mod world;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use clap::{Parser, Subcommand};
use comemo::Track;
use typst::engine::{Engine, Route, Sink, Traced};
use typst::foundations::StyleChain;
use typst::introspection::{EmptyIntrospector, Introspector, MAX_ITERS};
use typst::utils::Protected;
use typst::visualize::Color;
use typst::World;
use typst_kit::fonts::FontStore;
use typst_layout::PagedDocument;

use crate::diff::DiffOptions;
use crate::world::SimpleWorld;

/// Compares two versions of a Typst document and produces an annotated PDF
/// (additions in underlined blue, deletions in struck-through red).
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Launch a graphical interface to configure and run a diff, instead
    /// of using CLI arguments. Ignores any subcommand given alongside it
    #[arg(long)]
    gui: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Diff two separate `.typ` files (optionally full separate project
    /// directories) -- the default way to use typst-diff
    Files(FilesArgs),
    /// Diff the same `.typ` file across two revisions (tags, branches, or
    /// commits) of one local git repository, without checking either one
    /// out
    Git(GitArgs),
}

/// Every field is `pub(crate)`, not just as-needed by `main.rs` itself, so
/// `gui.rs` can build one of these directly from widget state and hand it
/// to `run_files`/`run_git` -- the exact same entry points the CLI itself
/// uses, so the two can't drift apart.
#[derive(clap::Args)]
pub(crate) struct FilesArgs {
    /// Old version of the `.typ` file
    pub(crate) old: PathBuf,
    /// New version of the `.typ` file
    pub(crate) new: PathBuf,
    /// Output PDF file
    #[arg(default_value = "diff.pdf")]
    pub(crate) output: PathBuf,
    /// Project root for the old file, used to resolve its absolute paths
    /// (`/lib/helpers.typ`, `json("/data.json")`...). Defaults to the old
    /// file's parent directory -- pass this explicitly when it lives in a
    /// subdirectory of the actual project root
    #[arg(long)]
    pub(crate) old_root: Option<PathBuf>,
    /// Project root for the new file (see `--old-root`)
    #[arg(long)]
    pub(crate) new_root: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

#[derive(clap::Args)]
pub(crate) struct GitArgs {
    /// Path to the local git repository (its working directory, or a bare
    /// repository)
    pub(crate) repo: PathBuf,
    /// Path to the `.typ` entry point, relative to the repository root --
    /// the same path is read at both revisions
    pub(crate) file: PathBuf,
    /// Output PDF file
    #[arg(default_value = "diff.pdf")]
    pub(crate) output: PathBuf,
    /// Old revision: a tag, branch, or commit -- anything `git rev-parse`
    /// would also accept (`v1.0`, `main`, `a1b2c3d`, `HEAD~3`...)
    #[arg(long)]
    pub(crate) old_rev: String,
    /// New revision (see `--old-rev`)
    #[arg(long)]
    pub(crate) new_rev: String,
    /// Project root for the old revision, used to resolve `FILE`'s
    /// absolute paths (`/lib/helpers.typ`, `json("/data.json")`...) --
    /// like `files` mode's `--old-root`, but a path relative to the
    /// *repository's* root (nothing is checked out, so there's no real
    /// filesystem directory to point at). Defaults to `FILE`'s own parent
    /// directory; pass `.` for the repository's own root itself
    #[arg(long)]
    pub(crate) old_root: Option<PathBuf>,
    /// Project root for the new revision (see `--old-root`)
    #[arg(long)]
    pub(crate) new_root: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) common: CommonArgs,
}

/// Flags shared identically by both `files` and `git` mode.
#[derive(clap::Args)]
pub(crate) struct CommonArgs {
    /// Don't show deleted content at all (by default, it's struck through
    /// in red)
    #[arg(long)]
    pub(crate) hide_deletions: bool,
    /// Don't highlight added content (by default, it's underlined in blue);
    /// when set, new content is rendered in standard style
    #[arg(long)]
    pub(crate) hide_additions: bool,
    /// Color deleted content is struck through in. Either one of Typst's
    /// named colors (red, orange, yellow, olive, green, lime, aqua, teal,
    /// eastern, navy, blue, purple, fuchsia, maroon, black, gray, silver,
    /// white) or a hex color (`#f30`, `7a03c2`, `abcdefff`)
    #[arg(long, default_value = "red", value_parser = parse_color)]
    pub(crate) deletion_color: Color,
    /// Color added content is underlined in. Same accepted forms as
    /// `--deletion-color`
    #[arg(long, default_value = "blue", value_parser = parse_color)]
    pub(crate) addition_color: Color,
    /// Additional directory to search recursively for fonts, on top of the
    /// ones embedded in the compiler. Can be passed multiple times; shared
    /// by both versions of the document
    #[arg(long = "font-path", value_name = "DIR")]
    pub(crate) font_paths: Vec<PathBuf>,
    /// Local directory packages (`@preview/cuti:0.4.0`,
    /// `@local/callout:0.1.0`, any namespace...) are resolved from,
    /// structured the way Typst's own package cache is
    /// (`<package-path>/<namespace>/<name>/<version>/...`, e.g.
    /// `@preview/cuti:0.4.0` resolves to
    /// `<package-path>/preview/cuti/0.4.0`). No package is ever downloaded
    /// from the network -- only one already present on disk under this
    /// directory (e.g. one `typst-cli` itself already cached, copied over,
    /// or one placed here by hand) can be resolved
    #[arg(long, value_name = "DIR")]
    pub(crate) package_path: Option<PathBuf>,
}

impl Default for CommonArgs {
    /// Starting point for `gui.rs`'s form state -- the same defaults the
    /// CLI's `#[arg(default_value = ...)]`/`Option::None` give when a flag
    /// is omitted.
    fn default() -> Self {
        Self {
            hide_deletions: false,
            hide_additions: false,
            deletion_color: Color::RED,
            addition_color: Color::BLUE,
            font_paths: Vec::new(),
            package_path: None,
        }
    }
}

/// Parses a color the way `--deletion-color`/`--addition-color` accept it:
/// one of Typst's named colors (case-insensitive), or a hex color in any
/// form `Color`'s own parser accepts (`#f30`, `7a03c2`, `abcdefff`...).
///
/// Typst resolves names like `red` as ordinary identifiers looked up in
/// the standard library's scope, which isn't something to reach for just
/// to parse one color, so this only mirrors the fixed list of named
/// color *constants* the library defines (`Color::RED` etc.) -- anything
/// fancier (`color.mix(...)`, `oklch(...)`, a named color not in this
/// list) needs a hex code instead.
fn parse_color(s: &str) -> Result<Color, String> {
    let color = match s.to_ascii_lowercase().as_str() {
        "black" => Some(Color::BLACK),
        "gray" | "grey" => Some(Color::GRAY),
        "white" => Some(Color::WHITE),
        "silver" => Some(Color::SILVER),
        "navy" => Some(Color::NAVY),
        "blue" => Some(Color::BLUE),
        "aqua" => Some(Color::AQUA),
        "teal" => Some(Color::TEAL),
        "eastern" => Some(Color::EASTERN),
        "purple" => Some(Color::PURPLE),
        "fuchsia" => Some(Color::FUCHSIA),
        "maroon" => Some(Color::MAROON),
        "red" => Some(Color::RED),
        "orange" => Some(Color::ORANGE),
        "yellow" => Some(Color::YELLOW),
        "olive" => Some(Color::OLIVE),
        "green" => Some(Color::GREEN),
        "lime" => Some(Color::LIME),
        _ => None,
    };
    if let Some(color) = color {
        return Ok(color);
    }
    s.parse::<Color>().map_err(|err| {
        format!("{s:?} is not a known color name, and not a valid hex color ({err})")
    })
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.gui {
        return gui::run();
    }
    match cli.command {
        Some(Command::Files(args)) => run_files(args),
        Some(Command::Git(args)) => run_git(args),
        None => anyhow::bail!("no subcommand given -- use `files`, `git`, or `--gui`; see --help"),
    }
}

pub(crate) fn run_files(args: FilesArgs) -> Result<()> {
    let (world_old, world_new) = build_files_worlds(&args)?;
    let document = diff_and_layout(&world_old, &world_new, &args.common)?;
    write_pdf(&document, &args.output)
}

pub(crate) fn run_git(args: GitArgs) -> Result<()> {
    let (world_old, world_new) = build_git_worlds(&args)?;
    let document = diff_and_layout(&world_old, &world_new, &args.common)?;
    write_pdf(&document, &args.output)
}

/// Builds the "old"/"new" `SimpleWorld` pair for `files` mode -- the
/// first half of [`run_files`], split out so `gui.rs`'s preview (which
/// needs the laid-out `PagedDocument`, via [`diff_and_layout`], but not a
/// PDF written to disk) can reuse it too.
pub(crate) fn build_files_worlds(args: &FilesArgs) -> Result<(SimpleWorld, SimpleWorld)> {
    // Canonicalized (absolute, symlink-resolved) so that, whatever form
    // the user spelled `old`/`new`/`--old-root`/`--new-root` in, the main
    // file path always lexically prefixes its project root the same way
    // -- required by `VirtualPath::virtualize` in `world.rs`.
    let old_main = canonicalize(&args.old)?;
    let new_main = canonicalize(&args.new)?;
    let old_root = resolve_root(&old_main, args.old_root.as_deref())?;
    let new_root = resolve_root(&new_main, args.new_root.as_deref())?;
    let package_path = resolve_package_path(args.common.package_path.as_deref())?;
    let fonts = build_fonts(&args.common.font_paths);

    // One Typst "world" per version: each has its own in-memory source,
    // rooted at its own project root.
    let world_old = SimpleWorld::from_directory(&old_main, &old_root, fonts.clone(), package_path.clone())?;
    let world_new = SimpleWorld::from_directory(&new_main, &new_root, fonts, package_path)?;
    Ok((world_old, world_new))
}

/// Like [`build_files_worlds`], but for `git` mode -- the first half of
/// [`run_git`].
pub(crate) fn build_git_worlds(args: &GitArgs) -> Result<(SimpleWorld, SimpleWorld)> {
    let repo_path = canonicalize(&args.repo)?;
    let package_path = resolve_package_path(args.common.package_path.as_deref())?;
    let fonts = build_fonts(&args.common.font_paths);

    // One Typst "world" per revision: each reads the same `FILE` (and
    // whatever it `#include`s/`#import`s) straight out of git's object
    // database at its own revision, without checking either one out.
    let world_old = SimpleWorld::from_git(
        &repo_path,
        &args.old_rev,
        &args.file,
        args.old_root.as_deref(),
        fonts.clone(),
        package_path.clone(),
    )?;
    let world_new = SimpleWorld::from_git(
        &repo_path,
        &args.new_rev,
        &args.file,
        args.new_root.as_deref(),
        fonts,
        package_path,
    )?;
    Ok((world_old, world_new))
}

/// Fonts are a project-wide setting (not specific to either version of the
/// document being diffed), and scanning `--font-path` directories is real
/// work -- gathered once here and shared (via `Arc::clone`) between the
/// "old" and "new" world instead of redoing it twice.
fn build_fonts(font_paths: &[PathBuf]) -> Arc<FontStore> {
    let mut font_store = FontStore::new();
    font_store.extend(typst_kit::fonts::embedded());
    for font_path in font_paths {
        font_store.extend(typst_kit::fonts::scan(font_path));
    }
    Arc::new(font_store)
}

/// Canonicalizes `--package-path`, if given.
fn resolve_package_path(package_path: Option<&Path>) -> Result<Option<PathBuf>> {
    package_path
        .map(|path| {
            path.canonicalize()
                .with_context(|| format!("resolving package path {path:?}"))
        })
        .transpose()
}

/// Diffs the two worlds and writes the resulting annotated PDF to `output`
/// -- the shared tail end of both `run_files` and `run_git`, once each has
/// built its own pair of `SimpleWorld`s.
/// Diffs the two worlds and lays out the result, without exporting it to
/// PDF yet -- the shared middle of `run_files`/`run_git` (which then call
/// [`write_pdf`]) and `gui.rs`'s preview (which rasterizes the returned
/// `PagedDocument` straight into a texture instead, with no PDF or file
/// on disk involved at all).
pub(crate) fn diff_and_layout(
    world_old: &SimpleWorld,
    world_new: &SimpleWorld,
    common: &CommonArgs,
) -> Result<PagedDocument> {
    let content_old = eval_to_content(world_old)?;
    let content_new = eval_to_content(world_new)?;

    // Computes the annotated Content (a "track changes"-style diff).
    let diff_options = DiffOptions {
        show_deletions: !common.hide_deletions,
        show_additions: !common.hide_additions,
        deletion_color: common.deletion_color.to_vec4_u8(),
        addition_color: common.addition_color.to_vec4_u8(),
    };
    let annotated = diff::diff_content(&content_old, &content_new, diff_options);

    // `#set page(header: ..., footer: ...)` (and any other page-construction
    // property) is deliberately left out of `annotated` above -- see
    // `collect()`'s `StyledElem` case in diff.rs for why carrying it along
    // per atom, like an ordinary style, would fragment the document into
    // one page per changed word. It's diffed and reapplied here instead,
    // once, on top of the whole document.
    let page_styles = diff::diff_page_marginalia(&content_old, &content_new, diff_options);
    let annotated = annotated.styled_with_map(page_styles);

    // Lays out this annotated content, reusing the "world" of the new
    // version (for fonts, the standard library, etc.)
    layout(world_new, &annotated)
}

/// Exports an already laid-out document to PDF and writes it to `output`
/// -- the second half of `run_files`/`run_git`, once each has built its
/// own [`diff_and_layout`] result.
pub(crate) fn write_pdf(document: &PagedDocument, output: &Path) -> Result<()> {
    let pdf_options = typst_pdf::PdfOptions::default();
    let pdf_bytes = typst_pdf::pdf(document, &pdf_options)
        .map_err(|errs| anyhow::anyhow!("PDF export error: {errs:?}"))?;

    std::fs::write(output, pdf_bytes).with_context(|| format!("writing {output:?}"))?;

    println!("Diff PDF written to {output:?}");
    Ok(())
}

/// Canonicalizes a path (resolves it to an absolute path with symlinks and
/// `.`/`..` components resolved away), with a friendly error on failure.
fn canonicalize(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .with_context(|| format!("reading {path:?}"))
}

/// Resolves the real project root for an already-canonicalized `.typ` file
/// path: either the explicit `--old-root`/`--new-root` the user passed
/// (canonicalized too), or, by default, the file's own parent directory.
fn resolve_root(main_path: &Path, explicit_root: Option<&Path>) -> Result<PathBuf> {
    match explicit_root {
        Some(root) => root
            .canonicalize()
            .with_context(|| format!("resolving project root {root:?}")),
        None => Ok(main_path
            .parent()
            .expect("a canonical file path has a parent")
            .to_path_buf()),
    }
}

/// Evaluates a world's source file and returns the resolved `Content`
/// (functions executed, variables substituted), without layout.
///
/// This is a SIMPLIFIED version of the internal `compile_impl` of the
/// `typst` crate (crates/typst/src/lib.rs): we only do a single evaluation
/// pass, without the "introspection" stabilization loop (counters, table of
/// contents...) -- that loop is what `layout()`, below, ports instead,
/// since that's where introspection-dependent content (a page counter, a
/// TOC entry...) actually gets resolved. A single evaluation pass remains
/// fine for the vast majority of documents, whose *evaluated content*
/// doesn't itself depend on page counts or element positions -- just not
/// a full port of `compile_impl`'s loop (which re-evaluates together with
/// re-laying-out) for the rare document whose content does.
fn eval_to_content(w: &SimpleWorld) -> Result<typst::foundations::Content> {
    let world = (w as &dyn World).track();
    let traced = Traced::default();
    let mut sink = Sink::new();
    let route = Route::default();
    let source = w.main_source();

    let module = typst_eval::eval(
        world,
        w.library(),
        traced.track(),
        sink.track_mut(),
        route.track(),
        source,
    )
    .map_err(|errs| anyhow::anyhow!("evaluation error: {errs:?}"))?;

    Ok(module.content())
}

/// Lays out an already-resolved `Content`, producing a `PagedDocument`
/// ready to be exported to PDF.
///
/// Mirrors the "introspection" stabilization loop in the `typst` crate's
/// own `compile_impl` (crates/typst/src/lib.rs), which `eval_to_content`'s
/// doc comment above flags as the piece this project's single-pass
/// simplification leaves out: a first pass lays out the document with no
/// knowledge yet of its own page count or content locations (an
/// `EmptyIntrospector`) -- fine for most content, but not for anything
/// that looks itself up during layout, like `#counter(page).final()` (how
/// many pages does the finished document have?) or a table of contents.
/// Each following pass re-lays out the SAME content, this time informed by
/// the `Introspector` the previous pass produced (which already knows
/// where everything ended up), until a pass's result no longer depends on
/// anything that changed since the one before it -- `constraint.validate`
/// is comemo's mechanism for detecting exactly that, the same one
/// `compile_impl` checks -- or `MAX_ITERS` passes have been tried without
/// converging, matching Typst's own give-up point.
fn layout(w: &SimpleWorld, content: &typst::foundations::Content) -> Result<PagedDocument> {
    let world = (w as &dyn World).track();
    let traced = Traced::default();
    let library = w.library();
    let styles = StyleChain::new(&library.styles);
    let empty_introspector = EmptyIntrospector;

    let mut previous: Option<PagedDocument> = None;
    for _ in 0..MAX_ITERS {
        let introspector: &dyn Introspector = match &previous {
            Some(doc) => doc.introspector().as_ref(),
            None => &empty_introspector,
        };
        let constraint = comemo::Constraint::new();

        let mut sink = Sink::new();
        let mut engine = Engine {
            world,
            library,
            introspector: Protected::new(introspector.track_with(&constraint)),
            traced: traced.track(),
            sink: sink.track_mut(),
            route: Route::default(),
        };

        let document = typst_layout::layout_document(&mut engine, content, styles)
            .map_err(|errs| anyhow::anyhow!("layout error: {errs:?}"))?;

        let document_introspector: &dyn Introspector = document.introspector().as_ref();
        if constraint.validate(document_introspector) {
            return Ok(document);
        }
        previous = Some(document);
    }

    Ok(previous.expect("MAX_ITERS > 0, so at least one layout pass ran above"))
}
