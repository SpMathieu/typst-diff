//! Cell-body diffs for unchanged table topology, plus common container descent.
//!
//! No JSON parsing here: JSON-backed tables have already been evaluated.
//! Clone NEW wrappers and cells, changing their bodies only. Inherited styles
//! are inspected to resolve geometry but are never replayed on tokens.
//! Structural row edits retain the previous simple-table fallback, guarded
//! against merged or positioned cells. This is not a row-ID matching engine.

use super::*;
use typst::layout::{AlignElem, BlockBody, BlockElem, BoxElem, PadElem};
use typst::model::FigureElem;

/// Structural anchors for a secondary alignment within a Myers Replace.
/// Same kinds and counts are paired by order, not by JSON record identity.
pub(super) fn kind(atom: &Atom) -> Option<&'static str> {
    let Atom::Leaf(c, _) = atom else { return None; };
    macro_rules! kinds {
        ($($ty:ty => $name:literal),+ $(,)?) => {
            $(if c.is::<$ty>() { return Some($name); })+
        };
    }
    kinds!(
        TableElem => "table", FigureElem => "figure", BlockElem => "block",
        BoxElem => "box", AlignElem => "align", PadElem => "pad",
        HeadingElem => "heading", StrongElem => "strong",
        EmphElem => "emph", LinkElem => "link",
    );
    None
}

pub(super) fn recurse(old: &Atom, new: &Atom, options: DiffOptions) -> Option<Content> {
    let (Atom::Leaf(a, sa), Atom::Leaf(b, sb)) = (old, new) else { return None; };
    if a.is::<TableElem>() && b.is::<TableElem>() {
        if let Some(table) = same_topology(a, b, sa, sb, options) {
            return Some(table);
        }
        // Preserve the older row-count-changing feature only where its flat
        // chunks(columns) model is actually valid. Never feed spans to it.
        if let Some(table) = simple_row_fallback(a, b, sa, sb, options) {
            return Some(table);
        }
        eprintln!(
            "typst-diff: table topology changed or cannot be paired safely; \
             falling back to whole-table replacement (cell counts, columns, \
             positions, spans, header groups, or separators differ)."
        );
        return None;
    }

    // Required Content bodies. Do not require a shared word: even a one-cell
    // table or a fully changed wrapper body still has a valid structural pair.
    macro_rules! body {
        ($ty:ty) => {
            if let (Some(a), Some(b)) = (a.to_packed::<$ty>(), b.to_packed::<$ty>()) {
                let mut result = b.clone();
                result.body = scoped::diff_content_in(&a.body, &b.body, sa, sb, options);
                return Some(result.pack());
            }
        };
    }
    body!(HeadingElem);
    body!(StrongElem);
    body!(EmphElem);
    body!(LinkElem);
    body!(FigureElem); // Caption and numbering follow NEW unchanged.
    body!(AlignElem);
    body!(PadElem);

    if let (Some(a), Some(b)) = (a.to_packed::<BoxElem>(), b.to_packed::<BoxElem>()) {
        let old_body = a.body.get_cloned(StyleChain::new(sa)).unwrap_or_default();
        let new_body = b.body.get_cloned(StyleChain::new(sb)).unwrap_or_default();
        let mut result = b.clone();
        result.body.set(Some(scoped::diff_content_in(
            &old_body, &new_body, sa, sb, options,
        )));
        return Some(result.pack());
    }
    if let (Some(a), Some(b)) = (a.to_packed::<BlockElem>(), b.to_packed::<BlockElem>()) {
        let extract = |body: Option<BlockBody>| match body {
            None => Some(Content::empty()),
            Some(BlockBody::Content(body)) => Some(body),
            // Layout callbacks are NOT evaluated here.
            _ => None,
        };
        let old_body = extract(a.body.get_cloned(StyleChain::new(sa)))?;
        let new_body = extract(b.body.get_cloned(StyleChain::new(sb)))?;
        let mut result = b.clone();
        result.body.set(Some(BlockBody::Content(scoped::diff_content_in(
            &old_body, &new_body, sa, sb, options,
        ))));
        return Some(result.pack());
    }
    None
}

fn columns(table: &Packed<TableElem>, styles: &Styles) -> usize {
    table.columns.get_ref(StyleChain::new(styles)).0.len().max(1)
}

fn compatible_cell(
    a: &Packed<TableCell>, b: &Packed<TableCell>, sa: &Styles, sb: &Styles,
) -> bool {
    let ca = StyleChain::new(sa);
    let cb = StyleChain::new(sb);
    a.x.get(ca) == b.x.get(cb)
        && a.y.get(ca) == b.y.get(cb)
        && a.colspan.get(ca) == b.colspan.get(cb)
        && a.rowspan.get(ca) == b.rowspan.get(cb)
}

fn diff_item(
    old: &TableItem, new: &TableItem, sa: &Styles, sb: &Styles, options: DiffOptions,
) -> Option<TableItem> {
    match (old, new) {
        (TableItem::Cell(a), TableItem::Cell(b)) => {
            if !compatible_cell(a, b, sa, sb) { return None; }
            let mut cell = b.clone();
            cell.body = scoped::diff_content_in(&a.body, &b.body, sa, sb, options);
            Some(TableItem::Cell(cell))
        }
        // Lines stay at their original place among NEW's children. Keeping
        // these nodes is safe here because no rows/cells are inserted.
        (TableItem::HLine(_), TableItem::HLine(_))
        | (TableItem::VLine(_), TableItem::VLine(_)) => Some(new.clone()),
        _ => None,
    }
}

fn diff_items(
    old: &[TableItem], new: &[TableItem], sa: &Styles, sb: &Styles, options: DiffOptions,
) -> Option<Vec<TableItem>> {
    if old.len() != new.len() { return None; }
    old.iter().zip(new).map(|(a, b)| diff_item(a, b, sa, sb, options)).collect()
}

fn diff_child(
    old: &TableChild, new: &TableChild, sa: &Styles, sb: &Styles, options: DiffOptions,
) -> Option<TableChild> {
    match (old, new) {
        (TableChild::Item(a), TableChild::Item(b)) => {
            diff_item(a, b, sa, sb, options).map(TableChild::Item)
        }
        (TableChild::Header(a), TableChild::Header(b)) => {
            if a.level.get(StyleChain::new(sa)) != b.level.get(StyleChain::new(sb)) {
                return None;
            }
            let mut header = b.clone();
            header.children = diff_items(&a.children, &b.children, sa, sb, options)?;
            Some(TableChild::Header(header))
        }
        (TableChild::Footer(a), TableChild::Footer(b)) => {
            let mut footer = b.clone();
            footer.children = diff_items(&a.children, &b.children, sa, sb, options)?;
            Some(TableChild::Footer(footer))
        }
        _ => None,
    }
}

fn same_topology(
    old: &Content, new: &Content, sa: &Styles, sb: &Styles, options: DiffOptions,
) -> Option<Content> {
    let a = old.to_packed::<TableElem>()?;
    let b = new.to_packed::<TableElem>()?;
    if columns(a, sa) != columns(b, sb) || a.children.len() != b.children.len() {
        return None;
    }
    let children = a.children.iter().zip(&b.children)
        .map(|(a, b)| diff_child(a, b, sa, sb, options))
        .collect::<Option<Vec<_>>>()?;
    let mut result = b.clone();
    result.children = children;
    // This code runs before synthesis. Invalidate a cached grid defensively.
    result.grid = None;
    Some(result.pack())
}

/// Legacy path ONLY for rectangular, automatically positioned tables with
/// changed row counts, plain cells, and at most a leading header/trailing footer.
/// The original row-by-position semantics (not stable record IDs) are retained.
fn simple_row_fallback(
    old: &Content, new: &Content, sa: &Styles, sb: &Styles, options: DiffOptions,
) -> Option<Content> {
    let a = old.to_packed::<TableElem>()?;
    let b = new.to_packed::<TableElem>()?;
    let count = columns(a, sa);
    if count != columns(b, sb) { return None; }
    fn plain(cell: &Packed<TableCell>, styles: &Styles) -> bool {
        let s = StyleChain::new(styles);
        cell.x.get(s).is_auto() && cell.y.get(s).is_auto()
            && cell.colspan.get(s).get() == 1 && cell.rowspan.get(s).get() == 1
    }
    fn valid(table: &Packed<TableElem>, styles: &Styles, count: usize) -> bool {
        table.children.iter().enumerate().all(|(i, child)| match child {
            TableChild::Item(TableItem::Cell(cell)) => plain(cell, styles),
            TableChild::Header(h) if i == 0 => h.children.len() % count == 0
                && h.children.iter().all(|item| matches!(item, TableItem::Cell(c) if plain(c, styles))),
            TableChild::Footer(f) if i + 1 == table.children.len() => f.children.len() % count == 0
                && f.children.iter().all(|item| matches!(item, TableItem::Cell(c) if plain(c, styles))),
            _ => false,
        })
    }
    if !valid(a, sa, count) || !valid(b, sb, count) { return None; }
    let ca = table_cells(&a.children)?;
    let cb = table_cells(&b.children)?;
    if ca.len() == cb.len() || ca.len() % count != 0 || cb.len() % count != 0 {
        return None;
    }
    // Materialize ONLY the column value needed by the old helper; don't
    // reapply inherited style maps. Wrappers in NEW still own the styles.
    let mut aa = a.clone();
    let mut bb = b.clone();
    aa.columns.set(a.columns.get_cloned(StyleChain::new(sa)));
    bb.columns.set(b.columns.get_cloned(StyleChain::new(sb)));
    // The legacy helper requires an explicitly nonempty column definition.
    if aa.columns.get_ref(StyleChain::new(sa)).0.is_empty() { return None; }
    let result = recurse_into_table(
        &Atom::Leaf(aa.pack(), Styles::new()),
        &Atom::Leaf(bb.pack(), Styles::new()), options,
    );
    if result.is_some() {
        eprintln!("typst-diff: row counts differ; using the legacy positional row diff (not JSON IDs).");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroUsize;
    use typst::layout::{Sizing, TrackSizings};
    use typst::model::{ParElem, TableFooter, TableHeader, TableHLine, TableVLine};

    fn tracks(n: usize) -> TrackSizings {
        TrackSizings((0..n).map(|_| Sizing::Auto).collect())
    }
    fn item(text: &str) -> TableItem {
        TableItem::Cell(Packed::new(TableCell::new(TextElem::packed(text.to_owned()))))
    }
    fn table(values: &[&str], n: usize) -> Content {
        TableElem::new(values.iter().map(|v| TableChild::Item(item(v))).collect())
            .with_columns(tracks(n)).pack()
    }
    fn diff(a: &Content, b: &Content) -> Content {
        scoped::diff_content_scoped(a, b, DiffOptions::default())
    }
    fn visit(c: &Content, f: &mut impl FnMut(&Content)) {
        f(c);
        if let Some(n) = c.to_packed::<SequenceElem>() {
            for child in &n.children { visit(child, f); }
        } else if let Some(n) = c.to_packed::<StyledElem>() {
            visit(&n.child, f);
        } else if let Some(n) = c.to_packed::<TableElem>() {
            for child in &n.children {
                match child {
                    TableChild::Item(TableItem::Cell(cell)) => visit(&cell.body, f),
                    TableChild::Header(h) => {
                        for item in &h.children {
                            if let TableItem::Cell(cell) = item { visit(&cell.body, f); }
                        }
                    }
                    TableChild::Footer(h) => {
                        for item in &h.children {
                            if let TableItem::Cell(cell) = item { visit(&cell.body, f); }
                        }
                    }
                    _ => {}
                }
            }
        } else if let Some(n) = c.to_packed::<BlockElem>() {
            if let Some(BlockBody::Content(body)) = n.body.get_cloned(StyleChain::default()) {
                visit(&body, f);
            }
        } else if let Some(n) = c.to_packed::<BoxElem>() {
            if let Some(body) = n.body.get_cloned(StyleChain::default()) { visit(&body, f); }
        } else {
            macro_rules! body {
                ($ty:ty) => {
                    if let Some(n) = c.to_packed::<$ty>() { visit(&n.body, f); return; }
                };
            }
            body!(StrikeElem); body!(UnderlineElem); body!(FigureElem);
            body!(AlignElem); body!(PadElem); body!(StrongElem);
            body!(EmphElem); body!(HeadingElem); body!(LinkElem);
        }
    }
    fn tables(c: &Content) -> Vec<Packed<TableElem>> {
        let mut result = Vec::new();
        visit(c, &mut |c| {
            if let Some(t) = c.to_packed::<TableElem>() { result.push(t.clone()); }
        });
        result
    }
    fn marks(c: &Content) -> (usize, usize) {
        let mut result = (0, 0);
        visit(c, &mut |c| {
            result.0 += usize::from(c.is::<StrikeElem>());
            result.1 += usize::from(c.is::<UnderlineElem>());
        });
        result
    }
    fn text(c: &Content) -> String {
        let mut out = String::new();
        visit(c, &mut |c| {
            if let Some(t) = c.to_packed::<TextElem>() { out.push_str(t.text.as_str()); }
        });
        out
    }
    fn cells(t: &Packed<TableElem>) -> Vec<Packed<TableCell>> {
        t.children.iter().filter_map(|c| match c {
            TableChild::Item(TableItem::Cell(c)) => Some(c.clone()), _ => None,
        }).collect()
    }
    fn assert_one_changed_cell(result: &Content, changed: usize, total: usize) {
        let ts = tables(result);
        assert_eq!(ts.len(), 1, "table must not be duplicated");
        let cs = cells(&ts[0]);
        assert_eq!(cs.len(), total, "cells must not be inserted or removed");
        for (i, cell) in cs.iter().enumerate() {
            assert_eq!(marks(&cell.body), if i == changed { (1, 1) } else { (0, 0) });
        }
    }

    #[test]
    fn one_changed_value_only_annotates_its_cell() {
        let a = table(&["A", "12", "B", "30"], 2);
        let b = table(&["A", "15", "B", "30"], 2);
        let result = diff(&a, &b);
        assert_one_changed_cell(&result, 1, 4);
        let cs = cells(&tables(&result)[0]);
        assert_eq!(text(&cs[1].body), "1215");
        assert_eq!(text(&cs[3].body), "30");
    }

    #[test]
    fn inherited_columns_work_without_local_columns() {
        fn doc(value: &str) -> Content {
            TableElem::new(vec![TableChild::Item(item("A")), TableChild::Item(item(value))])
                .pack().styled(TableElem::columns.set(tracks(2)))
        }
        assert_one_changed_cell(&diff(&doc("12"), &doc("15")), 1, 2);
    }

    #[test]
    fn separators_remain_at_the_same_indices() {
        fn doc(value: &str) -> Content {
            TableElem::new(vec![
                TableChild::Item(TableItem::HLine(Packed::new(TableHLine::new()))),
                TableChild::Item(item("A")), TableChild::Item(item(value)),
                TableChild::Item(TableItem::VLine(Packed::new(TableVLine::new()))),
            ]).with_columns(tracks(2)).pack()
        }
        let result = diff(&doc("12"), &doc("15"));
        assert_one_changed_cell(&result, 1, 2);
        let ts = tables(&result);
        assert!(matches!(&ts[0].children[0], TableChild::Item(TableItem::HLine(_))));
        assert!(matches!(&ts[0].children[3], TableChild::Item(TableItem::VLine(_))));
    }

    #[test]
    fn header_and_footer_cells_are_diffed_too() {
        fn doc(head: &str, foot: &str) -> Content {
            TableElem::new(vec![
                TableChild::Header(Packed::new(TableHeader::new(vec![item(head)]))),
                TableChild::Item(item("unchanged")),
                TableChild::Footer(Packed::new(TableFooter::new(vec![item(foot)]))),
            ]).with_columns(tracks(1)).pack()
        }
        let result = diff(&doc("oldhead", "oldfoot"), &doc("newhead", "newfoot"));
        let ts = tables(&result);
        assert_eq!(ts.len(), 1);
        assert_eq!(marks(&result), (2, 2));
        assert_eq!(marks(&cells(&ts[0])[0].body), (0, 0));
        assert!(matches!(&ts[0].children[0], TableChild::Header(_)));
        assert!(matches!(&ts[0].children[2], TableChild::Footer(_)));
    }

    #[test]
    fn unchanged_colspan_does_not_require_a_multiple_of_columns() {
        fn doc(value: &str) -> Content {
            TableElem::new(vec![
                TableChild::Item(TableItem::Cell(Packed::new(
                    TableCell::new(TextElem::packed("Band"))
                        .with_colspan(NonZeroUsize::new(2).unwrap()),
                ))),
                TableChild::Item(item("A")), TableChild::Item(item(value)),
            ]).with_columns(tracks(2)).pack()
        }
        let result = diff(&doc("12"), &doc("15"));
        assert_one_changed_cell(&result, 2, 3);
        assert_eq!(cells(&tables(&result)[0])[0].colspan.get(StyleChain::default()).get(), 2);
    }

    #[test]
    fn unchanged_rowspan_is_preserved() {
        fn doc(value: &str) -> Content {
            TableElem::new(vec![
                TableChild::Item(TableItem::Cell(Packed::new(
                    TableCell::new(TextElem::packed("Group"))
                        .with_rowspan(NonZeroUsize::new(2).unwrap()),
                ))),
                TableChild::Item(item(value)), TableChild::Item(item("second row")),
            ]).with_columns(tracks(2)).pack()
        }
        assert_one_changed_cell(&diff(&doc("12"), &doc("15")), 1, 3);
    }

    #[test]
    fn single_column_replacement_does_not_duplicate_the_row() {
        assert_one_changed_cell(&diff(&table(&["12"], 1), &table(&["15"], 1)), 0, 1);
    }

    #[test]
    fn a_fully_changed_row_stays_one_row() {
        let result = diff(&table(&["A", "12"], 2), &table(&["C", "15"], 2));
        assert_eq!(tables(&result).len(), 1);
        assert_eq!(cells(&tables(&result)[0]).len(), 2);
        assert_eq!(marks(&result), (2, 2));
    }

    #[test]
    fn nested_figure_block_align_pad_and_box_are_preserved() {
        fn doc(value: &str) -> Content {
            let t = table(&["A", value], 2);
            let b = BoxElem::new().with_body(Some(t)).pack();
            FigureElem::new(BlockElem::packed(AlignElem::new(PadElem::new(b).pack()).pack())).pack()
        }
        let result = diff(&doc("12"), &doc("15"));
        assert_one_changed_cell(&result, 1, 2);
        let mut count = 0;
        visit(&result, &mut |c| {
            count += usize::from(c.is::<FigureElem>() || c.is::<BlockElem>()
                || c.is::<AlignElem>() || c.is::<PadElem>() || c.is::<BoxElem>());
        });
        assert_eq!(count, 5);
    }

    #[test]
    fn inherited_geometry_survives_container_recursion() {
        fn doc(value: &str) -> Content {
            let t = TableElem::new(vec![TableChild::Item(item("A")), TableChild::Item(item(value))]).pack();
            BlockElem::packed(t).styled(TableElem::columns.set(tracks(2)))
        }
        assert_one_changed_cell(&diff(&doc("12"), &doc("15")), 1, 2);
    }

    #[test]
    fn adjacent_changed_tables_are_paired_inside_one_replace() {
        let a = Content::sequence([table(&["A", "12"], 2), table(&["B", "30"], 2)]);
        let b = Content::sequence([table(&["A", "15"], 2), table(&["B", "35"], 2)]);
        let result = diff(&a, &b);
        assert_eq!(tables(&result).len(), 2);
        for t in tables(&result) {
            assert_eq!(cells(&t).len(), 2);
            assert_eq!(marks(&cells(&t)[0].body), (0, 0));
            assert_eq!(marks(&cells(&t)[1].body), (1, 1));
        }
    }

    #[test]
    fn neighbouring_text_change_does_not_disable_table_recursion() {
        let a = Content::sequence([TextElem::packed("oldtitle"), table(&["A", "12"], 2)]);
        let b = Content::sequence([TextElem::packed("newtitle"), table(&["A", "15"], 2)]);
        let result = diff(&a, &b);
        assert_one_changed_cell(&result, 1, 2);
        assert_eq!(marks(&result), (2, 2));
    }

    #[test]
    fn nested_tables_are_diffed_recursively() {
        fn doc(value: &str) -> Content {
            TableElem::new(vec![TableChild::Item(TableItem::Cell(Packed::new(
                TableCell::new(table(&["A", value], 2)),
            )))]).with_columns(tracks(1)).pack()
        }
        let result = diff(&doc("12"), &doc("15"));
        assert_eq!(tables(&result).len(), 2);
        assert_eq!(marks(&result), (1, 1));
    }

    #[test]
    fn hide_flags_keep_the_new_cell_and_do_not_duplicate_table() {
        let a = table(&["A", "12"], 2);
        let b = table(&["A", "15"], 2);
        for (del, add) in [(false, true), (true, false), (false, false)] {
            let options = DiffOptions { show_deletions: del, show_additions: add, ..DiffOptions::default() };
            let result = scoped::diff_content_scoped(&a, &b, options);
            assert_eq!(tables(&result).len(), 1);
            assert_eq!(marks(&result), (usize::from(del), usize::from(add)));
            assert_eq!(text(&cells(&tables(&result)[0])[1].body), if del { "1215" } else { "15" });
        }
    }

    #[test]
    fn empty_cell_does_not_delete_or_shift_the_cell() {
        let result = diff(&table(&["A", "12", "B", "30"], 2), &table(&["A", "", "B", "30"], 2));
        let ts = tables(&result);
        assert_eq!(ts.len(), 1);
        let cs = cells(&ts[0]);
        assert_eq!(cs.len(), 4);
        assert_eq!(marks(&cs[1].body), (1, 0));
        assert_eq!(text(&cs[2].body), "B");
        assert_eq!(text(&cs[3].body), "30");
    }

    #[test]
    fn new_cell_attributes_are_kept() {
        let a = table(&["12"], 1);
        let mut b = table(&["15"], 1).into_packed::<TableElem>().unwrap();
        if let TableChild::Item(TableItem::Cell(cell)) = &mut b.children[0] {
            cell.fill.set(Smart::Custom(Some(Color::BLUE.into())));
        }
        let result = diff(&a, &b.clone().pack());
        let actual = cells(&tables(&result)[0])[0].fill.get_cloned(StyleChain::default());
        let expected = cells(&b)[0].fill.get_cloned(StyleChain::default());
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    }

    #[test]
    fn paragraph_styles_inside_cells_are_not_replayed_per_word() {
        fn doc(word: &str) -> Content {
            let body = TextElem::packed(format!("un {word} document"))
                .styled(ParElem::justify.set(true));
            TableElem::new(vec![TableChild::Item(TableItem::Cell(Packed::new(TableCell::new(body))))])
                .with_columns(tracks(1)).pack()
        }
        let result = diff(&doc("ancien"), &doc("nouveau"));
        let mut count = 0;
        visit(&result, &mut |c| {
            if let Some(s) = c.to_packed::<StyledElem>() {
                count += s.styles.iter().filter(|p| p.element() == Some(ParElem::ELEM)).count();
            }
        });
        assert_eq!(count, 1);
        assert_eq!(marks(&result), (1, 1));
    }

    #[test]
    fn incompatible_geometry_is_not_zipped_silently() {
        let styles = Styles::new();
        assert!(same_topology(&table(&["A", "12"], 1), &table(&["A", "15"], 2),
            &styles, &styles, DiffOptions::default()).is_none());
        assert!(same_topology(&table(&["A", "12"], 2), &table(&["A", "15", "B", "30"], 2),
            &styles, &styles, DiffOptions::default()).is_none());
    }

    #[test]
    fn unchanged_table_keeps_the_same_tree() {
        let a = table(&["A", "12", "B", "30"], 2);
        assert_eq!(format!("{:?}", diff(&a, &a)), format!("{a:?}"));
    }
}
