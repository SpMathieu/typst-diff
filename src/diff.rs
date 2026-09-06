//! Flattens a Typst `Content` into a sequence of comparable atoms,
//! computes a "track changes"-style diff, then rebuilds a `Content` with
//! deletions struck through in red and additions underlined in blue.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use similar::{capture_diff_slices, Algorithm, DiffOp};
use typst::foundations::{Content, NativeElement, SequenceElem, StyledElem, Styles};
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

/// Wraps a deleted atom: struck through and red, keeping its own styles
/// (e.g. weight, italics) for everything strikethrough/red doesn't
/// override.
///
/// The red fill is applied to the *base* content before the atom's own
/// styles are layered on top, so it ends up as the most specific (highest
/// priority) style — it's the whole point of the highlighting that it
/// stays visible no matter what color the surrounding document set.
fn wrap_deleted(atom: &Atom) -> Content {
    let content = atom_base_content(atom)
        .styled(TextElem::fill.set(Color::RED.into()))
        .styled_with_map(atom_styles(atom).clone());
    StrikeElem::new(content).pack()
}

/// Wraps an added atom: underlined and blue. See `wrap_deleted` for why
/// the color is applied before the atom's own styles.
fn wrap_added(atom: &Atom) -> Content {
    let content = atom_base_content(atom)
        .styled(TextElem::fill.set(Color::BLUE.into()))
        .styled_with_map(atom_styles(atom).clone());
    UnderlineElem::new(content).pack()
}

/// Controls how deletions and additions are rendered in the annotated
/// output.
#[derive(Clone, Copy)]
pub struct DiffOptions {
    /// If `false`, deleted content is dropped entirely instead of being
    /// shown struck through in red.
    pub show_deletions: bool,
    /// If `false`, added content is rendered in standard style (no
    /// underline, no color change) instead of underlined in blue.
    pub show_additions: bool,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self { show_deletions: true, show_additions: true }
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
            result.push(wrap_deleted(atom));
        }
    };
    let push_added = |result: &mut Vec<Content>, atom: &Atom| {
        result.push(if options.show_additions { wrap_added(atom) } else { atom_to_content(atom) });
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
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in 0..old_len {
                    push_deleted(&mut result, &atoms_old[old_index + i]);
                }
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in 0..new_len {
                    push_added(&mut result, &atoms_new[new_index + i]);
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                // A modification = deletion of the old + addition of the new.
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
