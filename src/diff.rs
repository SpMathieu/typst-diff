//! Flattens a Typst `Content` into a sequence of comparable atoms,
//! computes a "track changes"-style diff, then rebuilds a `Content` with
//! deletions struck through in red and additions underlined in blue.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use similar::{capture_diff_slices, Algorithm, DiffOp};
use typst::foundations::{Content, NativeElement, Packed, SequenceElem, StyledElem, Styles};
use typst::model::{
    EmphElem, HeadingElem, LinkElem, StrongElem, TableCell, TableChild, TableElem, TableItem,
};
use typst::text::{StrikeElem, TextElem, UnderlineElem};
use typst::visualize::Color;

/// A minimal, comparable unit of content, together with the styles
/// (`#set`/`#show` properties, e.g. color, weight, italics...) that were
/// applied to it via its ancestor `StyledElem`s in the document it came
/// from.
///
/// The styles are carried along purely to be *reapplied* when
/// reconstructing the annotated output (see `atom_to_content`) — they play
/// no part in matching atoms between the old and new document (see
/// `atom_key`), so a pure style change on otherwise-identical text is
/// still recognized as "the same atom" by the diff.
#[derive(Clone)]
pub enum Atom {
    /// A word or a punctuation mark.
    Word(String, Styles),
    /// A space or line break.
    Space(Styles),
    /// Any other "leaf" element (image, equation, etc.), treated as an
    /// indivisible atomic block.
    Leaf(Content, Styles),
}

/// Returns a canonical string used to compare atoms for equality/ordering.
///
/// `Content` doesn't implement `Eq`/`Hash`/`Ord` (it can hold floats and
/// other non-orderable data), so `Leaf` atoms are compared through their
/// `Debug` representation instead, which is a reasonable structural-equality
/// proxy for a diffing tool like this one. Styles are deliberately left out
/// of the key: two atoms with the same text/content but different styling
/// are still considered the same atom (see the `Atom` docs).
fn atom_key(atom: &Atom) -> String {
    match atom {
        Atom::Word(w, _) => format!("W:{w}"),
        Atom::Space(_) => "S".to_string(),
        Atom::Leaf(c, _) => format!("L:{c:?}"),
    }
}

impl PartialEq for Atom {
    fn eq(&self, other: &Self) -> bool {
        atom_key(self) == atom_key(other)
    }
}

impl Eq for Atom {}

impl Hash for Atom {
    fn hash<H: Hasher>(&self, state: &mut H) {
        atom_key(self).hash(state);
    }
}

impl PartialOrd for Atom {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Atom {
    fn cmp(&self, other: &Self) -> Ordering {
        atom_key(self).cmp(&atom_key(other))
    }
}

/// Recursively walks a `Content` and produces the flat list of its atoms,
/// in order of appearance, each carrying the styles in effect where it was
/// found.
pub fn flatten(content: &Content) -> Vec<Atom> {
    let mut atoms = Vec::new();
    collect(content, Styles::new(), &mut atoms);
    atoms
}

fn collect(content: &Content, styles: Styles, atoms: &mut Vec<Atom>) {
    if let Some(text) = content.to_packed::<TextElem>() {
        for token in tokenize(text.text.as_str(), &styles) {
            atoms.push(token);
        }
        return;
    }

    if let Some(seq) = content.to_packed::<SequenceElem>() {
        for child in &seq.children {
            collect(child, styles.clone(), atoms);
        }
        return;
    }

    if let Some(styled) = content.to_packed::<StyledElem>() {
        // Accumulate this level's styles into the ones inherited from
        // outer ancestors, with the same precedence a real style chain
        // would give them (this element's own styles are more specific,
        // so they must win over `styles` for any property both set) —
        // see `Styles::apply`'s doc/behavior for why the "outer" argument
        // ends up on the *lower*-precedence side of the merge.
        let mut merged = styled.styles.clone();
        merged.apply(styles);
        collect(&styled.child, merged, atoms);
        return;
    }

    // "Leaf" element: we don't know / don't want to descend any further
    // (image, equation, link, bold, italic...). It's treated as a whole
    // atomic block. This is a simplification: content INSIDE a
    // `strong(...)` or `emph(...)` is therefore not diffed word by word.
    // To refine this, add a case here for each element type you want to
    // "traverse" (StrongElem, EmphElem, LinkMarker, etc.), following the
    // SequenceElem/StyledElem cases above.
    atoms.push(Atom::Leaf(content.clone(), styles));
}

fn tokenize(s: &str, styles: &Styles) -> Vec<Atom> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !current.is_empty() {
                out.push(Atom::Word(std::mem::take(&mut current), styles.clone()));
            }
            out.push(Atom::Space(styles.clone()));
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        out.push(Atom::Word(current, styles.clone()));
    }
    out
}

/// The atom's own content, without its carried styles reapplied yet.
fn atom_base_content(atom: &Atom) -> Content {
    match atom {
        Atom::Word(w, _) => TextElem::packed(w.clone()),
        Atom::Space(_) => TextElem::packed(" "),
        Atom::Leaf(c, _) => c.clone(),
    }
}

/// The styles carried by the atom (see the `Atom` docs).
fn atom_styles(atom: &Atom) -> &Styles {
    match atom {
        Atom::Word(_, s) | Atom::Space(s) | Atom::Leaf(_, s) => s,
    }
}

/// Rebuilds an atom's content exactly as it appeared in its source
/// document, styles included.
fn atom_to_content(atom: &Atom) -> Content {
    atom_base_content(atom).styled_with_map(atom_styles(atom).clone())
}

/// Wraps a deleted atom: struck through, in `options`'s deletion color,
/// keeping its own styles (e.g. weight, italics) for everything the
/// strikethrough/color doesn't override.
///
/// The color is applied to the *base* content before the atom's own
/// styles are layered on top, so it ends up as the most specific (highest
/// priority) style — it's the whole point of the highlighting that it
/// stays visible no matter what color the surrounding document set.
fn wrap_deleted(atom: &Atom, options: DiffOptions) -> Content {
    let content = atom_base_content(atom)
        .styled(TextElem::fill.set(options.deletion_color().into()))
        .styled_with_map(atom_styles(atom).clone());
    StrikeElem::new(content).pack()
}

/// Wraps an added atom: underlined, in `options`'s addition color. See
/// `wrap_deleted` for why the color is applied before the atom's own
/// styles.
fn wrap_added(atom: &Atom, options: DiffOptions) -> Content {
    let content = atom_base_content(atom)
        .styled(TextElem::fill.set(options.addition_color().into()))
        .styled_with_map(atom_styles(atom).clone());
    UnderlineElem::new(content).pack()
}

/// Whether two pieces of content have at least one word (or other atom) in
/// common — spaces don't count, since they'd "match" regardless of the
/// words around them. Used by `try_recurse` to decide whether word-level
/// diffing is worth it at all for a given pair, versus falling back to a
/// clean whole-thing delete-then-insert (see its doc comment for why that
/// fallback matters). `recurse_into_table` needs a coarser, whole-cell
/// version of the same idea instead — see `row_shares_content`.
fn content_shares_a_word(old: &Content, new: &Content) -> bool {
    let atoms_old = flatten(old);
    let atoms_new = flatten(new);
    atoms_old
        .iter()
        .any(|atom| !matches!(atom, Atom::Space(_)) && atoms_new.contains(atom))
}

/// A "wrapper" element that holds one piece of inner content in a `body`
/// field: headings (`= Title`), `strong()`, `emph()`, and links all have
/// this shape. See `recurse_into_replaced` for why this matters.
trait BodyElement: NativeElement {
    fn body(&self) -> &Content;
    fn set_body(&mut self, body: Content);
}

/// Implements `BodyElement` for a list of types that all happen to have a
/// public `body: Content` field — true of every element listed below.
macro_rules! impl_body_element {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl BodyElement for $ty {
                fn body(&self) -> &Content { &self.body }
                fn set_body(&mut self, body: Content) { self.body = body; }
            }
        )+
    };
}

impl_body_element!(HeadingElem, StrongElem, EmphElem, LinkElem);

/// If `old_content` and `new_content` are both a `BodyElement` of the same
/// kind `E`, diffs their bodies word-by-word and rewraps the result in a
/// fresh `E` built from `new_content`'s own other attributes (heading
/// level/numbering, link destination...) — consistent with the "new
/// document wins" rule `diff_content` documents. Returns `None` if either
/// side isn't (or isn't the same kind of) `E`.
fn try_recurse<E: BodyElement>(
    old_content: &Content,
    new_content: &Content,
    options: DiffOptions,
) -> Option<Content> {
    let old_body = old_content.to_packed::<E>()?.body();
    let new_body = new_content.to_packed::<E>()?.body();

    // Only worth recursing if the two bodies actually share some word --
    // otherwise word-level diffing can scramble two completely unrelated
    // spans across each other (e.g. one name replaced by an unrelated one
    // splits into per-word deletes/inserts that interleave in a confusing
    // order), which reads far worse than a clean whole-span
    // delete-then-insert. Bailing out here to fall back to that is a
    // deliberate trade-off, not just an optimization.
    if !content_shares_a_word(old_body, new_body) {
        return None;
    }

    let diffed = diff_content(old_body, new_body, options);

    let mut rebuilt = new_content.clone().into_packed::<E>().ok()?;
    rebuilt.set_body(diffed);
    Some(rebuilt.pack())
}

/// When the top-level diff decided that one whole `Leaf` atom was replaced
/// by another (a straight 1-for-1 `Replace`), this recognizes cases where
/// both sides are actually the *same kind* of `BodyElement` — a heading,
/// `strong()`, `emph()`, or a link — and, rather than blindly swapping the
/// whole element, recurses the diff into their bodies word-by-word (see
/// `try_recurse`).
///
/// Without this, editing one word inside a heading's title, or inside
/// bold/italic/linked text, would strike through and re-underline the
/// *entire* span instead of just that word — the "treated as one atomic
/// block" limitation the README describes for `strong()`/`emph()`/links.
/// It can't just be fixed by traversing into these elements in `collect()`
/// the way `SequenceElem`/`StyledElem` are traversed, though: their
/// wrapper is what makes them render as bold/italic/a link/a heading at
/// all, so — unlike a `StyledElem`, which carries no rendering behavior of
/// its own beyond the styles `collect()` already carries along on each
/// atom — the wrapper has to be rebuilt around the diffed body, not
/// discarded while flattening.
///
/// Returns `None` when the pair isn't a kind of wrapper this recognizes,
/// so the caller falls back to the usual delete-then-insert. Add a case to
/// `impl_body_element!`'s list (and try it below) for other single-body
/// wrapper elements you want the same treatment for.
fn recurse_into_replaced(old: &Atom, new: &Atom, options: DiffOptions) -> Option<Content> {
    let Atom::Leaf(old_content, _) = old else {
        return None;
    };
    let Atom::Leaf(new_content, new_styles) = new else {
        return None;
    };

    let rebuilt = try_recurse::<HeadingElem>(old_content, new_content, options)
        .or_else(|| try_recurse::<StrongElem>(old_content, new_content, options))
        .or_else(|| try_recurse::<EmphElem>(old_content, new_content, options))
        .or_else(|| try_recurse::<LinkElem>(old_content, new_content, options))?;

    Some(rebuilt.styled_with_map(new_styles.clone()))
}

/// Wraps a table cell's body for deletion/addition, keeping the cell
/// itself (position, colspan/rowspan, alignment, fill...) intact so the
/// result is still a valid table cell — see `recurse_into_table`. Mirrors
/// `wrap_deleted`/`wrap_added`, just applied to `cell.body` in place of a
/// bare atom.
fn wrap_cell_deleted(cell: &Packed<TableCell>, options: DiffOptions) -> Packed<TableCell> {
    let mut rebuilt = cell.clone();
    rebuilt.body = StrikeElem::new(
        rebuilt
            .body
            .clone()
            .styled(TextElem::fill.set(options.deletion_color().into())),
    )
    .pack();
    rebuilt
}

fn wrap_cell_added(cell: &Packed<TableCell>, options: DiffOptions) -> Packed<TableCell> {
    let mut rebuilt = cell.clone();
    rebuilt.body = UnderlineElem::new(
        rebuilt
            .body
            .clone()
            .styled(TextElem::fill.set(options.addition_color().into())),
    )
    .pack();
    rebuilt
}

/// Splits a table's children into its plain data cells, in order —
/// `table.header`/`table.footer` children are left out (`recurse_into_table`
/// keeps those from the *new* table as-is, unchanged).
///
/// Returns `None` if the table also uses `table.hline`/`table.vline`:
/// diffing cell-by-cell doesn't know where a manually placed line should
/// end up once rows are added or removed around it, so tables using those
/// are intentionally left to the plain whole-table fallback instead.
fn table_cells(children: &[TableChild]) -> Option<Vec<Packed<TableCell>>> {
    let mut cells = Vec::new();
    for child in children {
        match child {
            TableChild::Header(_) | TableChild::Footer(_) => {}
            TableChild::Item(TableItem::Cell(cell)) => cells.push(cell.clone()),
            TableChild::Item(TableItem::HLine(_) | TableItem::VLine(_)) => return None,
        }
    }
    Some(cells)
}

/// The number of columns a table was explicitly given, if any.
///
/// `columns` is a settable (`#set table(columns: ...)`) property, not a
/// plain field, hence `.as_option()`. Returns `None` if it wasn't set
/// locally on this table (relying on an ambient `#set` from outside,
/// which isn't visible here — there's no style chain at this point in the
/// pipeline, only the bare `Content`) or is explicitly empty.
fn column_count(table: &Packed<TableElem>) -> Option<usize> {
    let n = table.columns.as_option().as_ref()?.0.len();
    (n > 0).then_some(n)
}

/// Whether two same-position rows share any *whole cell* — i.e. some cell
/// at the same column position is exactly identical on both sides.
///
/// This deliberately checks whole cells, not individual words within
/// them (unlike `content_shares_a_word`, which is the right granularity
/// for a run of prose): a table's cells are often short, formulaic values
/// that can share a trivial fragment — e.g. two unrelated percentages
/// like "3%" and "20%" both contain the literal "%" — without the row
/// actually having anything meaningful in common. Requiring a *whole*
/// matching cell avoids treating that kind of coincidence as a reason to
/// diff the row cell by cell instead of replacing it outright.
fn row_shares_content(old_row: &[Packed<TableCell>], new_row: &[Packed<TableCell>]) -> bool {
    old_row.iter().zip(new_row).any(|(old_cell, new_cell)| {
        format!("{:?}", old_cell.body) == format!("{:?}", new_cell.body)
    })
}

/// When the top-level diff decided that one whole table was replaced by
/// another (a straight 1-for-1 `Replace`), this compares their *rows*
/// (groups of `columns` consecutive cells) **position by position** — row
/// 1 of the old table against row 1 of the new one, row 2 against row 2,
/// and so on — instead of stacking the entire old table on top of the
/// entire new one.
///
/// Rows have to be the unit compared here, not individual cells: a table
/// has no explicit "row" grouping in its `Content` tree (cells are just a
/// flat list, wrapped into a grid `columns` cells at a time), so treating
/// individual cells as the unit being aligned, rather than whole rows,
/// can shift every cell after a change out of its column as soon as an
/// unequal number of cells needs replacing around one point.
///
/// Each pair of rows at the same position has its cells diffed one by one
/// (each cell recursed into via `diff_content`, so a single edited word is
/// still diffed word by word rather than swapping the whole cell) — *if*
/// the two rows share some content (like `try_recurse`'s guard on
/// `strong()`/`emph()`/links, applied here to a whole row instead of one
/// span of text). A row whose content is entirely unrelated to the row at
/// the same position on the other side (e.g. one whole record replaced by
/// an unrelated one) is instead shown as a clean whole-row deletion
/// immediately followed by a whole-row insertion, same as if it had no
/// counterpart at all — cell-by-cell diffing two unrelated rows would just
/// pair up coincidentally-placed cells with nothing to do with each other.
/// If the two tables have a different number of rows, the extra rows on
/// the longer side (always at the end, since rows before that are already
/// paired by position) are likewise shown as plain whole-row
/// deletions/insertions.
///
/// Returns `None` — falling back to the plain whole-table swap — when
/// either table doesn't have an explicit, equal `columns` count (see
/// `column_count`), or either uses `table.hline`/`table.vline`, or their
/// cells don't divide evenly into that many columns (see `table_cells`):
/// this covers the common case of a JSON-array-backed table gaining/losing
/// rows, not a full reimplementation of Typst's grid layout algorithm.
fn recurse_into_table(old: &Atom, new: &Atom, options: DiffOptions) -> Option<Content> {
    let Atom::Leaf(old_content, _) = old else {
        return None;
    };
    let Atom::Leaf(new_content, new_styles) = new else {
        return None;
    };

    let old_table = old_content.to_packed::<TableElem>()?;
    let new_table = new_content.to_packed::<TableElem>()?;
    let columns = column_count(old_table)?;
    if Some(columns) != column_count(new_table) {
        return None;
    }

    let old_cells = table_cells(&old_table.children)?;
    let new_cells = table_cells(&new_table.children)?;
    if old_cells.len() % columns != 0 || new_cells.len() % columns != 0 {
        return None;
    }
    let old_rows: Vec<&[Packed<TableCell>]> = old_cells.chunks(columns).collect();
    let new_rows: Vec<&[Packed<TableCell>]> = new_cells.chunks(columns).collect();

    // Headers/footers are kept from the new table, as-is (consistent with
    // "new document wins"), regardless of where they sat among the plain
    // cells originally — `table.header`/`table.footer` always repeat at
    // the top/bottom no matter their position among a table's children.
    let mut children: Vec<TableChild> = new_table
        .children
        .iter()
        .filter(|c| matches!(c, TableChild::Header(_)))
        .cloned()
        .collect();

    let push_deleted_row = |children: &mut Vec<TableChild>, row: &[Packed<TableCell>]| {
        if options.show_deletions {
            children.extend(
                row.iter()
                    .map(|cell| wrap_cell_deleted(cell, options))
                    .map(|cell| TableChild::Item(TableItem::Cell(cell))),
            );
        }
    };
    let push_added_row = |children: &mut Vec<TableChild>, row: &[Packed<TableCell>]| {
        let cells = row.iter().map(|cell| {
            if options.show_additions {
                wrap_cell_added(cell, options)
            } else {
                cell.clone()
            }
        });
        children.extend(cells.map(|cell| TableChild::Item(TableItem::Cell(cell))));
    };

    // Rows present on both sides, paired by position: diff each cell in
    // place rather than swapping the whole row -- unless the two rows
    // share nothing at all, in which case the old row is deleted and the
    // new one inserted right after it (see the doc comment above).
    let paired = old_rows.len().min(new_rows.len());
    for (old_row, new_row) in old_rows[..paired].iter().zip(&new_rows[..paired]) {
        if !row_shares_content(old_row, new_row) {
            push_deleted_row(&mut children, old_row);
            push_added_row(&mut children, new_row);
            continue;
        }

        let cells = old_row.iter().zip(*new_row).map(|(old_cell, new_cell)| {
            let mut cell = new_cell.clone();
            cell.body = diff_content(&old_cell.body, &new_cell.body, options);
            TableChild::Item(TableItem::Cell(cell))
        });
        children.extend(cells);
    }

    // Extra rows past the shorter table's length: whole-row
    // deletions/insertions (at most one of these loops actually runs).
    for row in &old_rows[paired..] {
        push_deleted_row(&mut children, row);
    }
    for row in &new_rows[paired..] {
        push_added_row(&mut children, row);
    }

    children.extend(
        new_table
            .children
            .iter()
            .filter(|c| matches!(c, TableChild::Footer(_)))
            .cloned(),
    );

    let mut rebuilt = new_content.clone().into_packed::<TableElem>().ok()?;
    rebuilt.children = children;
    Some(rebuilt.pack().styled_with_map(new_styles.clone()))
}

/// Controls how deletions and additions are rendered in the annotated
/// output.
///
/// Colors are stored as RGBA bytes rather than `Color` directly so this
/// stays `Copy` (`Color` isn't) — `deletion_color()`/`addition_color()`
/// convert back to a real `Color` on demand.
#[derive(Clone, Copy)]
pub struct DiffOptions {
    /// If `false`, deleted content is dropped entirely instead of being
    /// shown struck through.
    pub show_deletions: bool,
    /// If `false`, added content is rendered in standard style (no
    /// underline, no color change) instead of underlined.
    pub show_additions: bool,
    /// Color deleted content is struck through in. Defaults to Typst's
    /// `red`.
    pub deletion_color: [u8; 4],
    /// Color added content is underlined in. Defaults to Typst's `blue`.
    pub addition_color: [u8; 4],
}

impl DiffOptions {
    fn deletion_color(&self) -> Color {
        rgba(self.deletion_color)
    }

    fn addition_color(&self) -> Color {
        rgba(self.addition_color)
    }
}

fn rgba(c: [u8; 4]) -> Color {
    Color::from_u8(c[0], c[1], c[2], c[3])
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            show_deletions: true,
            show_additions: true,
            deletion_color: Color::RED.to_vec4_u8(),
            addition_color: Color::BLUE.to_vec4_u8(),
        }
    }
}

/// Computes the diff between two `Content`s and returns a new, annotated
/// `Content`.
///
/// By default, deletions are struck through in red and additions are
/// underlined in blue. `options` lets the caller turn either annotation off:
/// - hidden deletions are omitted from the output entirely,
/// - hidden additions are kept, but rendered in standard style.
///
/// Styling rule: the *new* document's style always wins. Unchanged text
/// (present in both versions) is rendered with the styles it has in the
/// new document, even if only its styling (not its text) changed — so a
/// pure style change is invisible in the diff (the "Known limitations"
/// section of the README explains why: `collect()` only recognizes atoms
/// as "the same" by their text/content, not by their styles), but at
/// least the surviving text always looks like the new document intends.
/// Deleted text (which has no counterpart in the new document) keeps
/// whatever styling it had in the old document.
pub fn diff_content(old: &Content, new: &Content, options: DiffOptions) -> Content {
    let atoms_old = flatten(old);
    let atoms_new = flatten(new);

    let ops = capture_diff_slices(Algorithm::Myers, &atoms_old, &atoms_new);

    let mut result = Vec::new();
    let push_deleted = |result: &mut Vec<Content>, atom: &Atom| {
        if options.show_deletions {
            result.push(wrap_deleted(atom, options));
        }
    };
    let push_added = |result: &mut Vec<Content>, atom: &Atom| {
        result.push(if options.show_additions {
            wrap_added(atom, options)
        } else {
            atom_to_content(atom)
        });
    };

    for op in ops {
        match op {
            DiffOp::Equal { new_index, len, .. } => {
                // Unchanged text still comes from the *new* document, so
                // that a pure style change (e.g. this run turning bold)
                // is reflected even though the diff doesn't flag it.
                for i in 0..len {
                    result.push(atom_to_content(&atoms_new[new_index + i]));
                }
            }
            DiffOp::Delete {
                old_index, old_len, ..
            } => {
                for i in 0..old_len {
                    push_deleted(&mut result, &atoms_old[old_index + i]);
                }
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                for i in 0..new_len {
                    push_added(&mut result, &atoms_new[new_index + i]);
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                // A straight 1-for-1 swap of two headings/strong()/emph()/
                // links/tables is diffed into its parts instead of struck
                // through/re-underlined whole (see `recurse_into_replaced`
                // and `recurse_into_table`).
                if old_len == 1 && new_len == 1 {
                    let recursed = recurse_into_replaced(
                        &atoms_old[old_index],
                        &atoms_new[new_index],
                        options,
                    )
                    .or_else(|| {
                        recurse_into_table(&atoms_old[old_index], &atoms_new[new_index], options)
                    });
                    if let Some(content) = recursed {
                        result.push(content);
                        continue;
                    }
                }

                // Otherwise, a modification = deletion of the old +
                // addition of the new.
                for i in 0..old_len {
                    push_deleted(&mut result, &atoms_old[old_index + i]);
                }
                for i in 0..new_len {
                    push_added(&mut result, &atoms_new[new_index + i]);
                }
            }
        }
    }

    Content::sequence(result)
}
