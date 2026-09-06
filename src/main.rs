mod diff;
mod world;

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use clap::Parser;
use comemo::Track;
use typst::engine::{Engine, Route, Sink, Traced};
use typst::foundations::StyleChain;
use typst::introspection::EmptyIntrospector;
use typst::utils::Protected;
use typst::World;
use typst_layout::PagedDocument;

use crate::diff::DiffOptions;
use crate::world::SimpleWorld;

/// Compares two versions of a Typst document and produces an annotated PDF
/// (additions in underlined blue, deletions in struck-through red).
#[derive(Parser)]
struct Args {
    /// Old version of the `.typ` file
    old: PathBuf,
    /// New version of the `.typ` file
    new: PathBuf,
    /// Output PDF file
    #[arg(default_value = "diff.pdf")]
    output: PathBuf,
    /// Don't show deleted content at all (by default, it's struck through
    /// in red)
    #[arg(long)]
    hide_deletions: bool,
    /// Don't highlight added content (by default, it's underlined in blue);
    /// when set, new content is rendered in standard style
    #[arg(long)]
    hide_additions: bool,
    /// Project root for the old file, used to resolve its absolute paths
    /// (`/lib/helpers.typ`, `json("/data.json")`...). Defaults to the old
    /// file's parent directory -- pass this explicitly when it lives in a
    /// subdirectory of the actual project root
    #[arg(long)]
    old_root: Option<PathBuf>,
    /// Project root for the new file (see `--old-root`)
    #[arg(long)]
    new_root: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Canonicalized (absolute, symlink-resolved) so that, whatever form
    // the user spelled `old`/`new`/`--old-root`/`--new-root` in, the main
    // file path always lexically prefixes its project root the same way
    // -- required by `VirtualPath::virtualize` in `world.rs`.
    let old_main = canonicalize(&args.old)?;
    let new_main = canonicalize(&args.new)?;
    let old_root = resolve_root(&old_main, args.old_root.as_deref())?;
    let new_root = resolve_root(&new_main, args.new_root.as_deref())?;

    // One Typst "world" per version: each has its own in-memory source,
    // rooted at its own project root.
    let world_old = SimpleWorld::new(&old_main, &old_root)?;
    let world_new = SimpleWorld::new(&new_main, &new_root)?;

    let content_old = eval_to_content(&world_old)?;
    let content_new = eval_to_content(&world_new)?;

    // Computes the annotated Content (a "track changes"-style diff).
    let diff_options = DiffOptions {
        show_deletions: !args.hide_deletions,
        show_additions: !args.hide_additions,
    };
    let annotated = diff::diff_content(&content_old, &content_new, diff_options);

    // Lays out this annotated content, reusing the "world" of the new
    // version (for fonts, the standard library, etc.)
    let document = layout(&world_new, &annotated)?;

    let pdf_options = typst_pdf::PdfOptions::default();
    let pdf_bytes = typst_pdf::pdf(&document, &pdf_options)
        .map_err(|errs| anyhow::anyhow!("PDF export error: {errs:?}"))?;

    std::fs::write(&args.output, pdf_bytes)
        .with_context(|| format!("writing {:?}", args.output))?;

    println!("Diff PDF written to {:?}", args.output);
    Ok(())
}

/// Canonicalizes a path (resolves it to an absolute path with symlinks and
/// `.`/`..` components resolved away), with a friendly error on failure.
fn canonicalize(path: &std::path::Path) -> Result<PathBuf> {
    path.canonicalize().with_context(|| format!("reading {path:?}"))
}

/// Resolves the real project root for an already-canonicalized `.typ` file
/// path: either the explicit `--old-root`/`--new-root` the user passed
/// (canonicalized too), or, by default, the file's own parent directory.
fn resolve_root(main_path: &std::path::Path, explicit_root: Option<&std::path::Path>) -> Result<PathBuf> {
    match explicit_root {
        Some(root) => root.canonicalize().with_context(|| format!("resolving project root {root:?}")),
        None => Ok(main_path.parent().expect("a canonical file path has a parent").to_path_buf()),
    }
}

/// Evaluates a world's source file and returns the resolved `Content`
/// (functions executed, variables substituted), without layout.
///
/// This is a SIMPLIFIED version of the internal `compile_impl` of the
/// `typst` crate (crates/typst/src/lib.rs): we only do a single evaluation
/// pass, without the "introspection" stabilization loop (counters, table of
/// contents...). This is enough for documents without complex cross
/// references; otherwise, the full `compile_impl` loop would need to be
/// ported.
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
fn layout(w: &SimpleWorld, content: &typst::foundations::Content) -> Result<PagedDocument> {
    let world = (w as &dyn World).track();
    let traced = Traced::default();
    let mut sink = Sink::new();
    let route = Route::default();
    let introspector = EmptyIntrospector;

    let mut engine = Engine {
        world,
        library: w.library(),
        introspector: Protected::new(introspector.track()),
        traced: traced.track(),
        sink: sink.track_mut(),
        route,
    };

    let library = w.library();
    let styles = StyleChain::new(&library.styles);

    typst_layout::layout_document(&mut engine, content, styles)
        .map_err(|errs| anyhow::anyhow!("layout error: {errs:?}"))
}
