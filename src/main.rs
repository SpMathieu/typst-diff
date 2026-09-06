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
}

fn main() -> Result<()> {
    let args = Args::parse();

    let old_text = std::fs::read_to_string(&args.old)
        .with_context(|| format!("reading {:?}", args.old))?;
    let new_text = std::fs::read_to_string(&args.new)
        .with_context(|| format!("reading {:?}", args.new))?;

    // One Typst "world" per version: each has its own in-memory source.
    let world_old = SimpleWorld::new(old_text);
    let world_new = SimpleWorld::new(new_text);

    let content_old = eval_to_content(&world_old)?;
    let content_new = eval_to_content(&world_new)?;

    // Computes the annotated Content (a "track changes"-style diff).
    let annotated = diff::diff_content(&content_old, &content_new);

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
