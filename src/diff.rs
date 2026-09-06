//! Flattens a Typst `Content` into a sequence of comparable atoms,
//! computes a "track changes"-style diff, then rebuilds a `Content` with
//! deletions struck through in red and additions underlined in blue.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use similar::{capture_diff_slices, Algorithm, DiffOp};
use typst::foundations::{Content, NativeElement, SequenceElem, StyledElem};
use typst::text::{StrikeElem, TextElem, UnderlineElem};
use typst::visualize::Color;

/// A minimal, comparable unit of content.
#[derive(Clone)]
pub enum Atom {
    /// A word or a punctuation mark.
    Word(String),
    /// A space or line break.
    Space,
    /// Any other "leaf" element (image, equation, etc.), treated as an
    /// indivisible atomic block.
    Leaf(Content),
}

/// Returns a canonical string used to compare atoms for equality/ordering.
///
/// `Content` doesn't implement `Eq`/`Hash`/`Ord` (it can hold floats and
/// other non-orderable data), so `Leaf` atoms are compared through their
/// `Debug` representation instead, which is a reasonable structural-equality
/// proxy for a diffing tool like this one.
fn atom_key(atom: &Atom) -> String {
    match atom {
        Atom::Word(w) => format!("W:{w}"),
        Atom::Space => "S".to_string(),
        Atom::Leaf(c) => format!("L:{c:?}"),
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
/// in order of appearance.
pub fn flatten(content: &Content) -> Vec<Atom> {
    let mut atoms = Vec::new();
    collect(content, &mut atoms);
    atoms
}

fn collect(content: &Content, atoms: &mut Vec<Atom>) {
    if let Some(text) = content.to_packed::<TextElem>() {
        for token in tokenize(text.text.as_str()) {
            atoms.push(token);
        }
        return;
    }

    if let Some(seq) = content.to_packed::<SequenceElem>() {
        for child in &seq.children {
            collect(child, atoms);
        }
        return;
    }

    if let Some(styled) = content.to_packed::<StyledElem>() {
        // Styles are ignored for now: we just descend into the child.
        // (Known limitation: a pure style change, without a text change,
        // won't be detected by this diff.)
        collect(&styled.child, atoms);
        return;
    }

    // "Leaf" element: we don't know / don't want to descend any further
    // (image, equation, link, bold, italic...). It's treated as a whole
    // atomic block. This is a simplification: content INSIDE a
    // `strong(...)` or `emph(...)` is therefore not diffed word by word.
    // To refine this, add a case here for each element type you want to
    // "traverse" (StrongElem, EmphElem, LinkMarker, etc.), following the
    // SequenceElem/StyledElem cases above.
    atoms.push(Atom::Leaf(content.clone()));
}

fn tokenize(s: &str) -> Vec<Atom> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !current.is_empty() {
                out.push(Atom::Word(std::mem::take(&mut current)));
            }
            out.push(Atom::Space);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        out.push(Atom::Word(current));
    }
    out
}

fn atom_to_content(atom: &Atom) -> Content {
    match atom {
        Atom::Word(w) => TextElem::packed(w.clone()),
        Atom::Space => TextElem::packed(" "),
        Atom::Leaf(c) => c.clone(),
    }
}

/// Wraps deleted content: struck through and red.
fn wrap_deleted(c: Content) -> Content {
    StrikeElem::new(c.clone().styled(TextElem::fill.set(Color::RED.into())))
        .pack()
}

/// Wraps added content: underlined and blue.
fn wrap_added(c: Content) -> Content {
    UnderlineElem::new(c.clone().styled(TextElem::fill.set(Color::BLUE.into())))
        .pack()
}

/// Computes the diff between two `Content`s and returns a new, annotated
/// `Content` (deletions struck through in red, additions underlined in
/// blue).
pub fn diff_content(old: &Content, new: &Content) -> Content {
    let atoms_old = flatten(old);
    let atoms_new = flatten(new);

    let ops = capture_diff_slices(Algorithm::Myers, &atoms_old, &atoms_new);

    let mut result = Vec::new();
    for op in ops {
        match op {
            DiffOp::Equal { old_index, len, .. } => {
                for i in 0..len {
                    result.push(atom_to_content(&atoms_old[old_index + i]));
                }
            }
            DiffOp::Delete { old_index, old_len, .. } => {
                for i in 0..old_len {
                    result.push(wrap_deleted(atom_to_content(&atoms_old[old_index + i])));
                }
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for i in 0..new_len {
                    result.push(wrap_added(atom_to_content(&atoms_new[new_index + i])));
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                // A modification = deletion of the old + addition of the new.
                for i in 0..old_len {
                    result.push(wrap_deleted(atom_to_content(&atoms_old[old_index + i])));
                }
                for i in 0..new_len {
                    result.push(wrap_added(atom_to_content(&atoms_new[new_index + i])));
                }
            }
        }
    }

    Content::sequence(result)
}
