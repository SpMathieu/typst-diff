//! Rebuild annotations inside the NEW content tree, instead of replaying
//! paragraph/page/show styles once per token.
//!
//! The outer template, page marginalia and style scopes are kept from NEW.
//! Tables, footnotes and common containers are diffed recursively. Deferred content and
//! unknown elements remain opaque.
//! Deleted text inherits the surrounding NEW style; old ancestor style maps
//! are intentionally not replayed (they could start paragraphs or pages).

use super::*;

#[derive(Default)]
struct Edit {
    before: Vec<Content>,
    added: bool,
    replacement: Option<Content>,
}

pub(super) fn diff_content_scoped(
    old: &Content,
    new: &Content,
    options: DiffOptions,
) -> Content {
    diff_content_in(old, new, &Styles::new(), &Styles::new(), options)
}

/// Inherited maps are READ for structural comparisons, never replayed in the
/// output. The caller's original new tree already supplies their style scope.
pub(super) fn diff_content_in(
    old: &Content,
    new: &Content,
    old_styles: &Styles,
    new_styles: &Styles,
    options: DiffOptions,
) -> Content {
    // This mode promises the new document, not a flattened approximation.
    if !options.show_deletions && !options.show_additions {
        return new.clone();
    }

    let mut old_atoms = Vec::new();
    let mut new_atoms = Vec::new();
    collect(old, old_styles.clone(), &mut old_atoms);
    collect(new, new_styles.clone(), &mut new_atoms);
    if old_atoms.iter().chain(&new_atoms).any(|atom| {
        matches!(atom, Atom::Leaf(c, _) if c.is::<typst::foundations::ContextElem>())
    }) {
        eprintln!(
            "typst-diff: warning: deferred context is opaque; changes inside it \
             may not be marked. The evaluated-tree diff is not a realized-content diff."
        );
    }
    let ops = capture_diff_slices(Algorithm::Myers, &old_atoms, &new_atoms);
    // The final slot holds deletions after the last new atom.
    let mut edits: Vec<Edit> = (0..=new_atoms.len()).map(|_| Edit::default()).collect();

    for op in ops {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete { old_index, old_len, new_index, .. } => {
                add_deletion(
                    &mut edits[new_index],
                    &old_atoms[old_index..old_index + old_len],
                    options,
                );
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for edit in &mut edits[new_index..new_index + new_len] {
                    edit.added = true;
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                replace_range(
                    &old_atoms[old_index..old_index + old_len],
                    &new_atoms[new_index..new_index + new_len],
                    new_index,
                    &mut edits,
                    options,
                );
            }
        }
    }

    let mut cursor = 0;
    let rebuilt = rebuild(new, &mut edits, &mut cursor, new_styles, options);
    assert_eq!(cursor, new_atoms.len(), "token traversal and reconstruction disagree");
    // Covers completely empty new content; normally the last leaf consumes it.
    let tail = std::mem::take(&mut edits[new_atoms.len()].before);
    if tail.is_empty() {
        rebuilt
    } else {
        Content::sequence(std::iter::once(rebuilt).chain(tail))
    }
}


/// A Myers replacement may contain words AND several changed containers.
/// Pair ordered structural anchors even when it is not a 1-for-1 operation.
/// Do not guess when their kinds or their counts differ.
fn replace_range(
    old: &[Atom],
    new: &[Atom],
    new_start: usize,
    edits: &mut [Edit],
    options: DiffOptions,
) {
    // Pair footnotes before generic structural anchors. This also handles a
    // changed note next to an inserted/deleted note when labels/content give
    // enough evidence, without imposing equal counts on the whole range.
    let notes = super::footnotes::pairs(old, new);
    if !notes.is_empty() {
        let (mut oi, mut ni) = (0, 0);
        for (o, n) in notes {
            replace_range(&old[oi..o], &new[ni..n], new_start + ni, edits, options);
            if let Some(content) = super::footnotes::recurse(&old[o], &new[n], options) {
                edits[new_start + n].replacement = Some(content);
            } else {
                plain_replacement(&old[o..o + 1], 1, new_start + n, edits, options);
            }
            oi = o + 1;
            ni = n + 1;
        }
        replace_range(&old[oi..], &new[ni..], new_start + ni, edits, options);
        return;
    }
    let a: Vec<_> = old.iter().enumerate()
        .filter_map(|(i, atom)| super::tables::kind(atom).map(|kind| (i, kind)))
        .collect();
    let b: Vec<_> = new.iter().enumerate()
        .filter_map(|(i, atom)| super::tables::kind(atom).map(|kind| (i, kind)))
        .collect();
    if !a.is_empty() && a.len() == b.len()
        && a.iter().zip(&b).all(|(a, b)| a.1 == b.1)
    {
        let mut oi = 0;
        let mut ni = 0;
        for ((o, _), (n, _)) in a.into_iter().zip(b) {
            plain_replacement(&old[oi..o], n - ni, new_start + ni, edits, options);
            if let Some(content) = super::tables::recurse(&old[o], &new[n], options) {
                edits[new_start + n].replacement = Some(content);
            } else {
                plain_replacement(&old[o..o + 1], 1, new_start + n, edits, options);
            }
            oi = o + 1;
            ni = n + 1;
        }
        plain_replacement(&old[oi..], new.len() - ni, new_start + ni, edits, options);
    } else {
        plain_replacement(old, new.len(), new_start, edits, options);
    }
}

fn plain_replacement(
    old: &[Atom],
    new_len: usize,
    new_start: usize,
    edits: &mut [Edit],
    options: DiffOptions,
) {
    add_deletion(&mut edits[new_start], old, options);
    for edit in &mut edits[new_start..new_start + new_len] {
        edit.added = true;
    }
}

fn add_deletion(edit: &mut Edit, atoms: &[Atom], options: DiffOptions) {
    edit.before.extend(super::footnotes::deleted(atoms, options));
}

fn addition(body: Content, added: bool, options: DiffOptions) -> Content {
    if added && options.show_additions {
        UnderlineElem::new(body.styled(TextElem::fill.set(options.addition_color().into())))
            .pack()
    } else {
        body
    }
}

fn append_tail(parts: &mut Vec<Content>, edits: &mut [Edit], cursor: usize) {
    if cursor == edits.len() - 1 {
        parts.append(&mut edits[cursor].before);
    }
}

fn rebuild(
    source: &Content,
    edits: &mut [Edit],
    cursor: &mut usize,
    styles: &Styles,
    options: DiffOptions,
) -> Content {
    if let Some(text) = source.to_packed::<TextElem>() {
        return rebuild_text(source, text.text.as_str(), edits, cursor, options);
    }
    if let Some(seq) = source.to_packed::<SequenceElem>() {
        if seq.children.is_empty() {
            let mut parts = Vec::new();
            append_tail(&mut parts, edits, *cursor);
            return if parts.is_empty() { source.clone() } else { Content::sequence(parts) };
        }
        let children = seq.children.iter()
            .map(|child| rebuild(child, edits, cursor, styles, options))
            .collect();
        let mut node = source.clone().into_packed::<SequenceElem>()
            .expect("source was a sequence");
        node.children = children;
        return node.pack();
    }
    if let Some(styled) = source.to_packed::<StyledElem>() {
        let mut inherited = strip_page_styles(styled.styles.clone());
        inherited.apply(styles.clone());
        let child = rebuild(&styled.child, edits, cursor, &inherited, options);
        let mut node = source.clone().into_packed::<StyledElem>()
            .expect("source was styled");
        node.child = child;
        // Preserve the original styles AND their location in the tree.
        return node.pack();
    }

    let edit = std::mem::take(&mut edits[*cursor]);
    *cursor += 1;
    let body = edit.replacement.unwrap_or_else(|| {
        if edit.added {
            super::footnotes::mark(source, styles, super::footnotes::Change::Added, options)
        } else { source.clone() }
    });
    let mut parts = edit.before;
    parts.push(body);
    append_tail(&mut parts, edits, *cursor);
    Content::sequence(parts)
}

// Same token boundaries as super::tokenize(), but retain the actual characters
// for reconstruction (including a non-breaking space rather than ASCII space).
fn text_tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if start < index {
                tokens.push(&text[start..index]);
            }
            let end = index + ch.len_utf8();
            tokens.push(&text[index..end]);
            start = end;
        }
    }
    if start < text.len() {
        tokens.push(&text[start..]);
    }
    tokens
}

fn flush_text(
    parts: &mut Vec<Content>,
    pending: &mut String,
    added: bool,
    source: &Content,
    options: DiffOptions,
) {
    if !pending.is_empty() {
        let body = TextElem::packed(std::mem::take(pending)).spanned(source.span());
        parts.push(addition(body, added, options));
    }
}

fn rebuild_text(
    source: &Content,
    text: &str,
    edits: &mut [Edit],
    cursor: &mut usize,
    options: DiffOptions,
) -> Content {
    let tokens = text_tokens(text);
    let end = *cursor + tokens.len();
    let unchanged = edits[*cursor..end].iter()
        .all(|edit| edit.before.is_empty() && !edit.added && edit.replacement.is_none());
    if unchanged {
        *cursor = end;
        let mut parts = vec![source.clone()];
        append_tail(&mut parts, edits, *cursor);
        return Content::sequence(parts);
    }

    let mut parts = Vec::new();
    let mut pending = String::new();
    let mut pending_added = false;
    for token in tokens {
        let edit = std::mem::take(&mut edits[*cursor]);
        *cursor += 1;
        if !edit.before.is_empty()
            || edit.replacement.is_some()
            || (!pending.is_empty() && pending_added != edit.added)
        {
            flush_text(&mut parts, &mut pending, pending_added, source, options);
        }
        parts.extend(edit.before);
        if let Some(replacement) = edit.replacement {
            parts.push(replacement);
        } else {
            pending_added = edit.added;
            pending.push_str(token);
        }
    }
    flush_text(&mut parts, &mut pending, pending_added, source, options);
    append_tail(&mut parts, edits, *cursor);
    Content::sequence(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use typst::model::ParElem;

    fn count_style<E: NativeElement>(content: &Content) -> usize {
        if let Some(node) = content.to_packed::<StyledElem>() {
            return node.styles.iter().filter(|s| s.element() == Some(E::ELEM)).count()
                + count_style::<E>(&node.child);
        }
        if let Some(node) = content.to_packed::<SequenceElem>() {
            return node.children.iter().map(count_style::<E>).sum();
        }
        if let Some(node) = content.to_packed::<StrikeElem>() {
            return count_style::<E>(&node.body);
        }
        if let Some(node) = content.to_packed::<UnderlineElem>() {
            return count_style::<E>(&node.body);
        }
        0
    }

    fn with_par(text: &str) -> Content {
        TextElem::packed(text.to_owned()).styled(ParElem::justify.set(true))
    }

    #[test]
    fn unchanged_tree_is_not_flattened() {
        let source = with_par("un document entier");
        let result = diff_content_scoped(&source, &source, DiffOptions::default());
        assert_eq!(format!("{result:?}"), format!("{source:?}"));
    }

    #[test]
    fn paragraph_style_is_not_replayed_per_word() {
        let result = diff_content_scoped(
            &with_par("un ancien document"),
            &with_par("un nouveau document"),
            DiffOptions::default(),
        );
        assert_eq!(count_style::<ParElem>(&result), 1);
    }

    #[test]
    fn clean_mode_returns_the_new_tree() {
        let new = with_par("le texte nouveau");
        let options = DiffOptions {
            show_deletions: false,
            show_additions: false,
            ..DiffOptions::default()
        };
        let result = diff_content_scoped(&with_par("ancien"), &new, options);
        assert_eq!(format!("{result:?}"), format!("{new:?}"));
    }

    #[test]
    fn page_styles_remain_in_their_two_separate_scopes() {
        fn document(word: &str) -> Content {
            Content::sequence([
                with_par(word).styled(PageElem::header.set(Smart::Custom(Some(
                    TextElem::packed("HEADER"),
                )))),
                with_par("annexe").styled(PageElem::header.set(Smart::Custom(None))),
            ])
        }
        let result = diff_content_scoped(
            &document("ancien"), &document("nouveau"), DiffOptions::default(),
        );
        assert_eq!(count_style::<PageElem>(&result), 2);
        let children = &result.to_packed::<SequenceElem>().unwrap().children;
        assert_eq!(children.len(), 2);
        for child in children {
            assert_eq!(count_style::<PageElem>(child), 1);
        }
    }

    #[test]
    fn trailing_deletion_stays_inside_the_new_scope() {
        let result = diff_content_scoped(
            &with_par("mot supprime"), &with_par("mot"), DiffOptions::default(),
        );
        assert!(result.to_packed::<StyledElem>().is_some());
        assert_eq!(count_style::<ParElem>(&result), 1);
    }

    #[test]
    fn whitespace_reconstruction_is_lossless() {
        let text = "un\u{00a0}texte\u{202f}!";
        assert_eq!(text_tokens(text).concat(), text);
        assert_eq!(text_tokens(text).len(), tokenize(text, &Styles::new()).len());
    }
}
