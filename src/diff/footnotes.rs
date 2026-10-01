//! Footnote diffs: one note per modification, annotations in its BODY, and
//! color-only markers (both the call and the default footnote entry).
//!
//! Numbering remains contextual: delegate to the original pattern/function,
//! then color its result. Do not hard-code counter values or add a global show
//! rule for footnote.entry. Inherited styles are read, never replayed per word.

use super::*;
use std::collections::BTreeSet;
use std::sync::LazyLock;
use comemo::Tracked;
use typst::diag::SourceResult;
use typst::engine::Engine;
use typst::foundations::{
    Args, Context, Func, IntoValue, Label, NativeFuncData, NativeFuncPtr, Reflect,
    Scope, Value,
};
use typst::layout::{AlignElem, BlockBody, BlockElem, BoxElem, PadElem};
use typst::model::{FigureElem, FootnoteBody, FootnoteElem, Numbering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Change { Added, Deleted }

/// Same source-level identity when it is known. A label reference is NOT a
/// declaration: retargeting a reference must not invent a new note body.
fn compatible(a: &Content, b: &Content) -> bool {
    let (Some(aa), Some(bb)) = (a.to_packed::<FootnoteElem>(), b.to_packed::<FootnoteElem>())
        else { return false; };
    if a.label() != b.label() { return false; }
    match (&aa.body, &bb.body) {
        (FootnoteBody::Content(_), FootnoteBody::Content(_)) => true,
        (FootnoteBody::Reference(x), FootnoteBody::Reference(y)) => x == y,
        _ => false,
    }
}

pub(super) fn recurse(old: &Atom, new: &Atom, options: DiffOptions) -> Option<Content> {
    let (Atom::Leaf(a, _sa), Atom::Leaf(b, sb)) = (old, new) else { return None; };
    if !compatible(a, b) { return None; }
    let aa = a.to_packed::<FootnoteElem>()?;
    let bb = b.to_packed::<FootnoteElem>()?;
    let (FootnoteBody::Content(old_body), FootnoteBody::Content(new_body)) = (&aa.body, &bb.body)
        else { return Some(b.clone()); };

    // Consistent with the parent diff: pure style changes aren't highlighted.
    let changed = flatten(old_body) != flatten(new_body);
    let mut note = bb.clone(); // Keeps NEW's label, source span and properties.
    note.body = FootnoteBody::Content(scoped::diff_content_scoped(
        old_body, new_body, options,
    ));
    if changed && options.show_additions {
        color_marker(&mut note, sb, options.addition_color());
    }
    Some(note.pack())
}

// One shared native function, with original numbering and color pre-applied.
// No leaked closure per note, no new crate dependency, no Typst source injection.
static COLORED_NUMBERING: NativeFuncData = NativeFuncData {
    function: NativeFuncPtr(&numbering_call),
    name: "__typst_diff_footnote_number",
    title: "Diff Footnote Number",
    docs: "Internal numbering adapter. Delegates before applying a text color.",
    def_site: None,
    keywords: &[],
    contextual: true,
    scope: LazyLock::new(&|| Scope::new()),
    params: LazyLock::new(&|| Vec::new()),
    returns: LazyLock::new(&|| <Content as Reflect>::output()),
};

fn numbering_call(
    engine: &mut Engine,
    context: Tracked<Context>,
    args: &mut Args,
) -> SourceResult<Value> {
    let original: Numbering = args.expect("original-numbering")?;
    let color: Color = args.expect("diff-color")?;
    let numbers: Vec<u64> = args.all()?;
    let output = original.apply(engine, context, args.span, &numbers)?.display();
    Ok(Value::Content(output.styled(TextElem::fill.set(color.into()))))
}

fn color_marker(note: &mut Packed<FootnoteElem>, styles: &Styles, color: Color) {
    let original = note.numbering.get_cloned(StyleChain::new(styles));
    let mut args = Args::new(note.span(), [original.into_value(), color.into_value()]);
    let numbering = Func::from(&COLORED_NUMBERING).spanned(note.span()).with(&mut args);
    // Store on the element: FootnoteEntry reads numbering without the call's
    // inherited StyleChain. An outer text(fill) would not color both numbers.
    note.numbering.set(Numbering::Func(numbering));
}

fn color(change: Change, options: DiffOptions) -> Color {
    match change {
        Change::Added => options.addition_color(),
        Change::Deleted => options.deletion_color(),
    }
}

fn decorate(body: Content, change: Change, options: DiffOptions) -> Content {
    let body = body.styled(TextElem::fill.set(color(change, options).into()));
    match change {
        Change::Added => UnderlineElem::new(body).pack(),
        Change::Deleted => StrikeElem::new(body).pack(),
    }
}

/// Whole addition/deletion, including notes INSIDE an added/deleted wrapper.
/// Keep page/paragraph scopes; never put a strike/underline around a FootnoteElem.
/// Unknown/deferred containers remain opaque, as in the preceding patches.
pub(super) fn mark(
    source: &Content, styles: &Styles, change: Change, options: DiffOptions,
) -> Content {
    if change == Change::Deleted && !options.show_deletions { return Content::empty(); }
    if change == Change::Added && !options.show_additions { return source.clone(); }
    if let Some(node) = source.to_packed::<SequenceElem>() {
        let mut out = node.clone();
        out.children = node.children.iter().map(|c| mark(c, styles, change, options)).collect();
        return out.pack();
    }
    if let Some(node) = source.to_packed::<StyledElem>() {
        let mut inherited = strip_page_styles(node.styles.clone());
        inherited.apply(styles.clone());
        let mut out = node.clone();
        out.child = mark(&node.child, &inherited, change, options);
        return out.pack();
    }
    if let Some(node) = source.to_packed::<FootnoteElem>() {
        let mut out = node.clone();
        if let FootnoteBody::Content(body) = &node.body {
            out.body = FootnoteBody::Content(mark(body, &Styles::new(), change, options));
        }
        color_marker(&mut out, styles, color(change, options));
        return out.pack();
    }
    macro_rules! body {
        ($ty:ty) => {
            if let Some(node) = source.to_packed::<$ty>() {
                let mut out = node.clone();
                out.body = mark(&node.body, styles, change, options);
                return out.pack();
            }
        };
    }
    body!(HeadingElem);
    body!(StrongElem);
    body!(EmphElem);
    body!(LinkElem);
    body!(FigureElem);
    body!(AlignElem);
    body!(PadElem);
    if let Some(node) = source.to_packed::<BoxElem>() {
        let body = node.body.get_cloned(StyleChain::new(styles));
        let mut out = node.clone();
        out.body.set(body.map(|c| mark(&c, styles, change, options)));
        return out.pack();
    }
    if let Some(node) = source.to_packed::<BlockElem>() {
        let body = node.body.get_cloned(StyleChain::new(styles));
        if let Some(BlockBody::Content(body)) = body {
            let mut out = node.clone();
            out.body.set(Some(BlockBody::Content(mark(&body, styles, change, options))));
            return out.pack();
        }
    }
    if let Some(node) = source.to_packed::<TableElem>() {
        fn item(item: &TableItem, s: &Styles, k: Change, o: DiffOptions) -> TableItem {
            match item {
                TableItem::Cell(cell) => {
                    let mut out = cell.clone();
                    out.body = mark(&cell.body, s, k, o);
                    TableItem::Cell(out)
                }
                _ => item.clone(),
            }
        }
        let mut out = node.clone();
        out.children = node.children.iter().map(|child| match child {
            TableChild::Item(c) => TableChild::Item(item(c, styles, change, options)),
            TableChild::Header(c) => {
                let mut h = c.clone();
                h.children = c.children.iter().map(|c| item(c, styles, change, options)).collect();
                TableChild::Header(h)
            }
            TableChild::Footer(c) => {
                let mut f = c.clone();
                f.children = c.children.iter().map(|c| item(c, styles, change, options)).collect();
                TableChild::Footer(f)
            }
        }).collect();
        out.grid = None;
        return out.pack();
    }
    decorate(source.clone(), change, options)
}

/// Coalesce plain deleted tokens, but treat every leaf separately so a note
/// doesn't get buried inside a single StrikeElem around an entire deletion.
pub(super) fn deleted(atoms: &[Atom], options: DiffOptions) -> Vec<Content> {
    if !options.show_deletions { return Vec::new(); }
    fn flush(out: &mut Vec<Content>, pending: &mut Vec<Content>, o: DiffOptions) {
        if !pending.is_empty() {
            out.push(decorate(Content::sequence(std::mem::take(pending)), Change::Deleted, o));
        }
    }
    let mut out = Vec::new();
    let mut pending = Vec::new();
    for atom in atoms {
        match atom {
            Atom::Leaf(c, styles) => {
                flush(&mut out, &mut pending, options);
                out.push(mark(c, styles, Change::Deleted, options));
            }
            _ => pending.push(atom_base_content(atom)),
        }
    }
    flush(&mut out, &mut pending, options);
    out
}

/// Ordered alignment of notes within a single Myers replacement. Stable labels
/// have priority. Unlabelled one-for-one notes are paired by their position;
/// with unequal counts, require lexical evidence rather than pairing blindly.
/// This does not track moved notes globally across independent diff hunks.
pub(super) fn pairs(old: &[Atom], new: &[Atom]) -> Vec<(usize, usize)> {
    struct Note<'a> {
        index: usize,
        content: &'a Content,
        label: Option<Label>,
        words: BTreeSet<String>,
    }
    fn notes(atoms: &[Atom]) -> Vec<Note<'_>> {
        atoms.iter().enumerate().filter_map(|(index, atom)| {
            let Atom::Leaf(content, _) = atom else { return None; };
            let node = content.to_packed::<FootnoteElem>()?;
            let words = node.body_content().map(|body| {
                body.plain_text().split_whitespace().map(str::to_owned).collect()
            }).unwrap_or_default();
            Some(Note { index, content, label: content.label(), words })
        }).collect()
    }
    let a = notes(old);
    let b = notes(new);
    if a.is_empty() || b.is_empty() { return Vec::new(); }
    if a.len() == b.len() && a.iter().zip(&b).all(|(x, y)| compatible(x.content, y.content)) {
        return a.iter().zip(&b).map(|(x, y)| (x.index, y.index)).collect();
    }
    // Bound auxiliary memory and work on pathological generated documents.
    if a.len().saturating_mul(b.len()) > 262_144 {
        eprintln!("typst-diff: too many ambiguous footnotes in one replacement; keeping explicit deletions/additions.");
        return Vec::new();
    }
    fn score(a: &Note<'_>, b: &Note<'_>) -> u64 {
        if !compatible(a.content, b.content) { return 0; }
        if a.label.is_some() { return 1_000_000; }
        if a.content.to_packed::<FootnoteElem>().unwrap().is_ref() { return 1_000_000; }
        let shared = a.words.intersection(&b.words).count();
        let total = a.words.len() + b.words.len();
        // Dice >= 1/2: a common article alone is not sufficient in a long note.
        if shared > 0 && 4 * shared >= total {
            1 + (200 * shared / total) as u64
        } else { 0 }
    }
    let width = b.len() + 1;
    let mut best = vec![0u64; (a.len() + 1) * width];
    let mut weights = vec![0u64; a.len() * b.len()];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            let w = score(&a[i], &b[j]);
            weights[i * b.len() + j] = w;
            best[i * width + j] = best[(i + 1) * width + j]
                .max(best[i * width + j + 1])
                .max(if w == 0 { 0 } else { w + best[(i + 1) * width + j + 1] });
        }
    }
    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let w = weights[i * b.len() + j];
        if w > 0 && best[i * width + j] == w + best[(i + 1) * width + j + 1] {
            result.push((a[i].index, b[j].index));
            i += 1;
            j += 1;
        } else if best[(i + 1) * width + j] > best[i * width + j + 1] {
            i += 1;
        } else { j += 1; }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::ControlFlow;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use typst::model::{NumberingPattern, ParElem};
    use typst_syntax::Span;

    fn note(s: &str) -> Content {
        FootnoteElem::with_content(TextElem::packed(s.to_owned())).pack()
    }
    fn diff(a: &Content, b: &Content, o: DiffOptions) -> Content {
        scoped::diff_content_scoped(a, b, o)
    }
    fn count<E: NativeElement>(c: &Content) -> usize {
        let mut n = 0;
        let _ = c.traverse(&mut |c| -> ControlFlow<()> {
            if c.is::<E>() { n += 1; }
            ControlFlow::Continue(())
        });
        n
    }
    fn notes(c: &Content) -> Vec<Packed<FootnoteElem>> {
        let mut notes = Vec::new();
        let _ = c.traverse(&mut |c| -> ControlFlow<()> {
            if let Some(n) = c.to_packed::<FootnoteElem>() { notes.push(n.clone()); }
            ControlFlow::Continue(())
        });
        notes
    }
    fn pattern(s: &str) -> Numbering {
        Numbering::Pattern(s.parse::<NumberingPattern>().unwrap())
    }
    fn assert_numbering(n: &Packed<FootnoteElem>, original: Numbering, color: Color) {
        let mut args = Args::new(Span::detached(), [original.into_value(), color.into_value()]);
        let expected = Numbering::Func(Func::from(&COLORED_NUMBERING).with(&mut args));
        assert_eq!(n.numbering.get_cloned(StyleChain::default()), expected);
    }
    fn assert_no_decorated_call(c: &Content) {
        // Decorations must contain text, never a footnote declaration.
        let _ = c.traverse(&mut |c| -> ControlFlow<()> {
            if let Some(n) = c.to_packed::<StrikeElem>() {
                assert_eq!(count::<FootnoteElem>(&n.body), 0);
            }
            if let Some(n) = c.to_packed::<UnderlineElem>() {
                assert_eq!(count::<FootnoteElem>(&n.body), 0);
            }
            ControlFlow::Continue(())
        });
    }

    #[test]
    fn changed_note_is_one_note_with_body_diff() {
        let o = DiffOptions::default();
        let r = diff(&note("une ancienne valeur"), &note("une nouvelle valeur"), o);
        let ns = notes(&r);
        assert_eq!(ns.len(), 1);
        let body = ns[0].body_content().unwrap();
        assert_eq!(count::<StrikeElem>(body), 1);
        assert_eq!(count::<UnderlineElem>(body), 1);
        assert!(body.plain_text().contains("ancienne"));
        assert!(body.plain_text().contains("nouvelle"));
        assert_numbering(&ns[0], pattern("1"), o.addition_color());
        assert_no_decorated_call(&r);
    }

    #[test]
    fn entirely_rewritten_note_still_has_one_number() {
        let r = diff(&note("alpha"), &note("omega"), DiffOptions::default());
        assert_eq!(notes(&r).len(), 1);
        assert_eq!(count::<StrikeElem>(&r), 1);
        assert_eq!(count::<UnderlineElem>(&r), 1);
    }

    #[test]
    fn deleting_words_inside_note_uses_addition_marker() {
        let o = DiffOptions::default();
        let r = diff(&note("texte detail superflu"), &note("texte detail"), o);
        let ns = notes(&r);
        assert_eq!(ns.len(), 1);
        assert_numbering(&ns[0], pattern("1"), o.addition_color());
        assert!(count::<StrikeElem>(ns[0].body_content().unwrap()) > 0);
        assert_no_decorated_call(&r);
    }

    #[test]
    fn emptying_note_is_not_removing_declaration() {
        let o = DiffOptions::default();
        let old = note("texte");
        let new = FootnoteElem::with_content(Content::empty()).pack();
        let r = diff(&old, &new, o);
        let ns = notes(&r);
        assert_eq!(ns.len(), 1);
        assert_numbering(&ns[0], pattern("1"), o.addition_color());
    }

    #[test]
    fn complete_deletion_keeps_deleted_body_and_red_number() {
        let o = DiffOptions::default();
        let r = diff(&note("ancienne note"), &Content::empty(), o);
        let ns = notes(&r);
        assert_eq!(ns.len(), 1);
        assert_numbering(&ns[0], pattern("1"), o.deletion_color());
        assert_eq!(count::<StrikeElem>(ns[0].body_content().unwrap()), 1);
        assert_eq!(count::<UnderlineElem>(&r), 0);
        assert_no_decorated_call(&r);
    }

    #[test]
    fn insertion_marks_body_and_number() {
        let o = DiffOptions::default();
        let r = diff(&Content::empty(), &note("nouvelle note"), o);
        let ns = notes(&r);
        assert_eq!(ns.len(), 1);
        assert_numbering(&ns[0], pattern("1"), o.addition_color());
        assert_eq!(count::<UnderlineElem>(ns[0].body_content().unwrap()), 1);
        assert_eq!(count::<StrikeElem>(&r), 0);
        assert_no_decorated_call(&r);
    }

    #[test]
    fn unchanged_note_is_not_colored() {
        let n = note("inchangee");
        let r = diff(&n, &n, DiffOptions::default());
        assert_eq!(format!("{r:?}"), format!("{n:?}"));
        assert_eq!(count::<StrikeElem>(&r) + count::<UnderlineElem>(&r), 0);
    }

    #[test]
    fn custom_colors_reach_numbering() {
        let o = DiffOptions {
            addition_color: [12, 120, 80, 255],
            deletion_color: [150, 30, 170, 255],
            ..DiffOptions::default()
        };
        let changed = diff(&note("ancien"), &note("nouveau"), o);
        assert_numbering(&notes(&changed)[0], pattern("1"), o.addition_color());
        let removed = diff(&note("supprime"), &Content::empty(), o);
        assert_numbering(&notes(&removed)[0], pattern("1"), o.deletion_color());
    }

    #[test]
    fn inherited_symbol_numbering_is_preserved_on_insertion() {
        let o = DiffOptions::default();
        let n = note("ajout").styled(FootnoteElem::numbering.set(pattern("*")));
        let r = diff(&Content::empty(), &n, o);
        assert_numbering(&notes(&r)[0], pattern("*"), o.addition_color());
    }

    #[test]
    fn inherited_numbering_is_preserved_on_modification_and_deletion() {
        let o = DiffOptions::default();
        let a = note("ancien").styled(FootnoteElem::numbering.set(pattern("a")));
        let b = note("nouveau").styled(FootnoteElem::numbering.set(pattern("i")));
        assert_numbering(&notes(&diff(&a, &b, o))[0], pattern("i"), o.addition_color());
        assert_numbering(&notes(&diff(&a, &Content::empty(), o))[0], pattern("a"), o.deletion_color());
    }

    #[test]
    fn hide_deletions_removes_whole_deleted_note() {
        let o = DiffOptions { show_deletions: false, ..DiffOptions::default() };
        let r = diff(&note("supprimee"), &Content::empty(), o);
        assert_eq!(count::<FootnoteElem>(&r), 0);
    }

    #[test]
    fn hide_deletions_keeps_modified_note_with_addition_color() {
        let o = DiffOptions { show_deletions: false, ..DiffOptions::default() };
        let r = diff(&note("ancien"), &note("nouveau"), o);
        assert_eq!(count::<StrikeElem>(&r), 0);
        assert_numbering(&notes(&r)[0], pattern("1"), o.addition_color());
    }

    #[test]
    fn hide_additions_keeps_regular_number_and_deleted_words() {
        let o = DiffOptions { show_additions: false, ..DiffOptions::default() };
        let r = diff(&note("ancien"), &note("nouveau"), o);
        assert_eq!(count::<UnderlineElem>(&r), 0);
        assert_eq!(count::<StrikeElem>(&r), 1);
        assert_eq!(notes(&r)[0].numbering.get_cloned(StyleChain::default()), pattern("1"));
    }

    #[test]
    fn clean_mode_returns_exact_new_tree() {
        let o = DiffOptions { show_additions: false, show_deletions: false, ..DiffOptions::default() };
        let new = note("nouveau").styled(ParElem::justify.set(true));
        let r = diff(&note("ancien"), &new, o);
        assert_eq!(format!("{r:?}"), format!("{new:?}"));
    }

    #[test]
    fn notes_inside_changed_table_cells_are_diffed() {
        fn table(n: Content) -> Content {
            TableElem::new(vec![TableChild::Item(TableItem::Cell(Packed::new(TableCell::new(n))))]).pack()
        }
        let r = diff(&table(note("ancien")), &table(note("nouveau")), DiffOptions::default());
        assert_eq!(count::<TableElem>(&r), 1);
        assert_eq!(count::<FootnoteElem>(&r), 1);
        assert_eq!(count::<StrikeElem>(&r), 1);
        assert_eq!(count::<UnderlineElem>(&r), 1);
    }

    #[test]
    fn note_inside_added_or_deleted_wrapper_has_annotated_body() {
        for change in [Change::Added, Change::Deleted] {
            let wrapper = StrongElem::new(Content::sequence([
                TextElem::packed("texte"), note("dans le conteneur"),
            ])).pack();
            let r = mark(&wrapper, &Styles::new(), change, DiffOptions::default());
            let ns = notes(&r);
            assert_eq!(ns.len(), 1);
            assert_eq!(count::<StrongElem>(&r), 1);
            assert_no_decorated_call(&r);
            let body = ns[0].body_content().unwrap();
            match change {
                Change::Added => assert_eq!(count::<UnderlineElem>(body), 1),
                Change::Deleted => assert_eq!(count::<StrikeElem>(body), 1),
            }
        }
    }

    #[test]
    fn replacement_with_neighboring_words_keeps_one_note() {
        let a = Content::sequence([TextElem::packed("alpha "), note("ancienne valeur")]);
        let b = Content::sequence([TextElem::packed("omega "), note("nouvelle valeur")]);
        let r = diff(&a, &b, DiffOptions::default());
        assert_eq!(count::<FootnoteElem>(&r), 1);
        assert_no_decorated_call(&r);
    }

    #[test]
    fn adjacent_modified_notes_are_paired_in_order() {
        let a = Content::sequence([note("alpha"), note("beta")]);
        let b = Content::sequence([note("gamma"), note("delta")]);
        let r = diff(&a, &b, DiffOptions::default());
        assert_eq!(count::<FootnoteElem>(&r), 2);
        assert_eq!(count::<StrikeElem>(&r), 2);
        assert_eq!(count::<UnderlineElem>(&r), 2);
    }

    #[test]
    fn unequal_note_counts_use_content_evidence() {
        let a = flatten(&note("quantite ancienne conservee"));
        let b = flatten(&Content::sequence([
            note("totalement autre sujet"), note("quantite nouvelle conservee"),
        ]));
        assert_eq!(pairs(&a, &b), vec![(0, 1)]);
    }

    #[test]
    fn unequal_unrelated_notes_are_not_associated_arbitrarily() {
        let a = flatten(&note("alpha"));
        let b = flatten(&Content::sequence([note("beta"), note("gamma")]));
        assert!(pairs(&a, &b).is_empty());
    }

    #[test]
    fn label_references_do_not_turn_into_new_definitions() {
        let label = Label::construct("existing".into()).unwrap();
        let reference = FootnoteElem::with_label(label).pack();
        let r = diff(&Content::empty(), &reference, DiffOptions::default());
        let ns = notes(&r);
        assert_eq!(ns.len(), 1);
        assert!(ns[0].is_ref());
        assert!(ns[0].body_content().is_none());
        assert_no_decorated_call(&r);
    }

    // These integration tests exercise the real evaluator, numbering callback
    // and layout loop. They are NOT PDF visual assertions. No network/packages.
    struct Project(PathBuf);
    impl Project {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "typst-diff-footnote-test-{}-{}-{}", std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
                NEXT.fetch_add(1, AtomicOrdering::Relaxed),
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
        fn world(&self, name: &str, text: &str) -> crate::world::SimpleWorld {
            let file = self.0.join(name);
            std::fs::write(&file, text).unwrap();
            crate::world::SimpleWorld::from_directory(
                &file, &self.0, crate::build_fonts(&[]), None,
            ).unwrap()
        }
    }
    impl Drop for Project {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn integration_numbering_callback_and_body_diff_layout() {
        let project = Project::new();
        for numbering in ["\"1\"", "\"*\"", "n => [N#n]"] {
            let setup = format!("#set footnote(numbering: {numbering})\n#set par(justify: true)\n");
            let old = project.world("old.typ", &format!("{setup}Texte#footnote[ancienne valeur]"));
            let new = project.world("new.typ", &format!("{setup}Texte#footnote[nouvelle valeur]"));
            let a = crate::eval_to_content(&old).unwrap();
            let b = crate::eval_to_content(&new).unwrap();
            let result = diff(&a, &b, DiffOptions::default());
            assert_eq!(count::<FootnoteElem>(&result), 1);
            assert!(!crate::layout(&new, &result).unwrap().pages.is_empty());
        }
    }

    #[test]
    fn integration_stable_labels_keep_modified_note_next_to_insertion() {
        let project = Project::new();
        let old = project.world("old.typ", "Texte#footnote[alpha]<stable>");
        let new = project.world("new.typ", "Texte#footnote[beta]<inserted>#footnote[gamma]<stable>");
        let a = crate::eval_to_content(&old).unwrap();
        let b = crate::eval_to_content(&new).unwrap();
        let result = diff(&a, &b, DiffOptions::default());
        assert_eq!(count::<FootnoteElem>(&result), 2);
        assert_eq!(count::<StrikeElem>(&result), 1);
        assert!(!crate::layout(&new, &result).unwrap().pages.is_empty());
    }

    #[test]
    fn integration_deleted_and_inserted_notes_layout_with_custom_colors() {
        let project = Project::new();
        let old = project.world("old.typ", "Avant#footnote[obsolete]. Fin.");
        let new = project.world("new.typ", "Avant. Fin#footnote[nouvelle].");
        let a = crate::eval_to_content(&old).unwrap();
        let b = crate::eval_to_content(&new).unwrap();
        let o = DiffOptions { addition_color: [0, 128, 70, 255], ..DiffOptions::default() };
        let result = diff(&a, &b, o);
        assert_no_decorated_call(&result);
        assert!(!crate::layout(&new, &result).unwrap().pages.is_empty());
    }
    fn frame_text(
        frame: &typst::layout::Frame,
        out: &mut Vec<(String, typst::visualize::Paint)>,
    ) {
        for (_, item) in frame.items() {
            match item {
                typst::layout::FrameItem::Group(g) => frame_text(&g.frame, out),
                typst::layout::FrameItem::Text(t) => out.push((t.text.to_string(), t.fill.clone())),
                _ => {}
            }
        }
    }

    #[test]
    fn integration_modified_note_colors_both_markers_but_not_unchanged_body() {
        let project = Project::new();
        let old = project.world("old.typ", "Texte#footnote[ancienne valeur]");
        let new = project.world("new.typ", "Texte#footnote[nouvelle valeur]");
        let options = DiffOptions::default();
        let result = diff(&crate::eval_to_content(&old).unwrap(),
                          &crate::eval_to_content(&new).unwrap(), options);
        let doc = crate::layout(&new, &result).unwrap();
        let mut spans = Vec::new();
        for page in &doc.pages { frame_text(&page.frame, &mut spans); }
        let markers: Vec<_> = spans.iter().filter(|(text, _)| text.trim() == "1").collect();
        assert_eq!(markers.len(), 2, "one call and one entry number, not two notes");
        let added: typst::visualize::Paint = options.addition_color().into();
        let deleted: typst::visualize::Paint = options.deletion_color().into();
        let normal: typst::visualize::Paint = Color::BLACK.into();
        assert!(markers.iter().all(|(_, fill)| *fill == added));
        assert!(spans.iter().any(|(text, fill)| text.contains("ancienne") && *fill == deleted));
        assert!(spans.iter().any(|(text, fill)| text.contains("nouvelle") && *fill == added));
        assert!(spans.iter().any(|(text, fill)| text.contains("valeur") && *fill == normal));
    }

    #[test]
    fn integration_whole_note_addition_and_deletion_color_both_markers() {
        let project = Project::new();
        for change in [Change::Added, Change::Deleted] {
            let (a, b) = match change {
                Change::Added => ("Texte", "Texte#footnote[note]"),
                Change::Deleted => ("Texte#footnote[note]", "Texte"),
            };
            let old = project.world("old.typ", a);
            let new = project.world("new.typ", b);
            let options = DiffOptions {
                addition_color: [10, 100, 60, 255],
                deletion_color: [180, 30, 90, 255],
                ..DiffOptions::default()
            };
            let result = diff(&crate::eval_to_content(&old).unwrap(),
                              &crate::eval_to_content(&new).unwrap(), options);
            let doc = crate::layout(&new, &result).unwrap();
            let mut spans = Vec::new();
            for page in &doc.pages { frame_text(&page.frame, &mut spans); }
            let expected: typst::visualize::Paint = color(change, options).into();
            let markers: Vec<_> = spans.iter().filter(|(text, _)| text.trim() == "1").collect();
            assert_eq!(markers.len(), 2);
            assert!(markers.iter().all(|(_, fill)| *fill == expected));
            assert!(spans.iter().any(|(text, fill)| text.contains("note") && *fill == expected));
        }
    }

}
