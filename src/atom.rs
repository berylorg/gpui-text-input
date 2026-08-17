use std::ops::Range;

use crate::boundary::clamp_to_char_boundary;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputAtom {
    pub(crate) id: String,
    pub(crate) range: Range<usize>,
    pub(crate) copy_text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputSelectionExport {
    display_text: String,
    copy_text: String,
    atoms: Vec<TextInputSelectionAtom>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextInputSelectionAtom {
    pub(crate) id: String,
    pub(crate) range: Range<usize>,
    pub(crate) display_text: String,
    pub(crate) copy_text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextInputAtomError {
    InvalidRange,
    OverlappingRange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OffsetAffinity {
    Backward,
    Forward,
    Nearest,
}

impl TextInputAtom {
    pub fn new(id: impl Into<String>, range: Range<usize>, copy_text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            range,
            copy_text: copy_text.into(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub fn copy_text(&self) -> &str {
        &self.copy_text
    }
}

impl TextInputSelectionExport {
    pub(crate) fn new(
        display_text: String,
        copy_text: String,
        atoms: Vec<TextInputSelectionAtom>,
    ) -> Self {
        Self {
            display_text,
            copy_text,
            atoms,
        }
    }

    pub fn display_text(&self) -> &str {
        &self.display_text
    }

    pub fn copy_text(&self) -> &str {
        &self.copy_text
    }

    pub fn atoms(&self) -> &[TextInputSelectionAtom] {
        &self.atoms
    }

    pub fn has_atoms(&self) -> bool {
        !self.atoms.is_empty()
    }
}

impl TextInputSelectionAtom {
    pub fn new(
        id: impl Into<String>,
        range: Range<usize>,
        display_text: impl Into<String>,
        copy_text: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            range,
            display_text: display_text.into(),
            copy_text: copy_text.into(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub fn display_text(&self) -> &str {
        &self.display_text
    }

    pub fn copy_text(&self) -> &str {
        &self.copy_text
    }
}

pub(crate) fn validate_atoms(
    text: &str,
    atoms: impl IntoIterator<Item = TextInputAtom>,
) -> Result<Vec<TextInputAtom>, TextInputAtomError> {
    let mut atoms = atoms.into_iter().collect::<Vec<_>>();
    atoms.sort_by_key(|atom| atom.range.start);

    let mut previous_end = 0usize;
    for atom in &atoms {
        if atom.range.start >= atom.range.end
            || atom.range.end > text.len()
            || !text.is_char_boundary(atom.range.start)
            || !text.is_char_boundary(atom.range.end)
        {
            return Err(TextInputAtomError::InvalidRange);
        }
        if atom.range.start < previous_end {
            return Err(TextInputAtomError::OverlappingRange);
        }
        previous_end = atom.range.end;
    }

    Ok(atoms)
}

pub(crate) fn validate_selection_atoms(
    text: &str,
    atoms: impl IntoIterator<Item = TextInputSelectionAtom>,
) -> Result<Vec<TextInputSelectionAtom>, TextInputAtomError> {
    let mut atoms = atoms.into_iter().collect::<Vec<_>>();
    atoms.sort_by_key(|atom| atom.range.start);

    let mut previous_end = 0usize;
    for atom in &atoms {
        if atom.range.start >= atom.range.end
            || atom.range.end > text.len()
            || !text.is_char_boundary(atom.range.start)
            || !text.is_char_boundary(atom.range.end)
            || &text[atom.range.clone()] != atom.display_text.as_str()
        {
            return Err(TextInputAtomError::InvalidRange);
        }
        if atom.range.start < previous_end {
            return Err(TextInputAtomError::OverlappingRange);
        }
        previous_end = atom.range.end;
    }

    Ok(atoms)
}

pub(crate) fn snap_offset_to_atoms(
    text: &str,
    atoms: &[TextInputAtom],
    offset: usize,
    affinity: OffsetAffinity,
) -> usize {
    let offset = clamp_to_char_boundary(text, offset);
    let Some(atom) = atoms
        .iter()
        .find(|atom| offset > atom.range.start && offset < atom.range.end)
    else {
        return offset;
    };

    match affinity {
        OffsetAffinity::Backward => atom.range.start,
        OffsetAffinity::Forward => atom.range.end,
        OffsetAffinity::Nearest => {
            if offset - atom.range.start < atom.range.end - offset {
                atom.range.start
            } else {
                atom.range.end
            }
        }
    }
}

pub(crate) fn normalize_selection_range_with_atoms(
    range: Range<usize>,
    text: &str,
    atoms: &[TextInputAtom],
    empty_affinity: OffsetAffinity,
) -> Range<usize> {
    let start = clamp_to_char_boundary(text, range.start);
    let end = clamp_to_char_boundary(text, range.end);
    let range = start.min(end)..start.max(end);

    if range.is_empty() {
        let offset = snap_offset_to_atoms(text, atoms, range.start, empty_affinity);
        return offset..offset;
    }

    expand_range_to_intersecting_atoms(range, text, atoms)
}

pub(crate) fn normalize_edit_range_with_atoms(
    range: Range<usize>,
    text: &str,
    atoms: &[TextInputAtom],
) -> Range<usize> {
    let start = clamp_to_char_boundary(text, range.start);
    let end = clamp_to_char_boundary(text, range.end);
    let range = start.min(end)..start.max(end);

    if range.is_empty() {
        let offset = snap_offset_to_atoms(text, atoms, range.start, OffsetAffinity::Nearest);
        return offset..offset;
    }

    expand_range_to_intersecting_atoms(range, text, atoms)
}

pub(crate) fn expand_range_to_intersecting_atoms(
    range: Range<usize>,
    text: &str,
    atoms: &[TextInputAtom],
) -> Range<usize> {
    let mut start = range.start;
    let mut end = range.end;

    for atom in atoms {
        if ranges_intersect(&(start..end), &atom.range) {
            start = start.min(atom.range.start);
            end = end.max(atom.range.end);
        }
    }

    clamp_to_char_boundary(text, start)..clamp_to_char_boundary(text, end)
}

pub(crate) fn range_intersects_atoms(range: &Range<usize>, atoms: &[TextInputAtom]) -> bool {
    atoms
        .iter()
        .any(|atom| ranges_intersect(range, &atom.range))
}

pub(crate) fn transform_atoms_after_edit(
    atoms: &[TextInputAtom],
    range: &Range<usize>,
    replacement_len: usize,
    mut inserted_atoms: Vec<TextInputAtom>,
) -> Vec<TextInputAtom> {
    let mut next_atoms = Vec::with_capacity(atoms.len() + inserted_atoms.len());

    for atom in atoms {
        if atom.range.end <= range.start {
            next_atoms.push(atom.clone());
        } else if atom.range.start >= range.end {
            let start = range.start + replacement_len + (atom.range.start - range.end);
            let end = range.start + replacement_len + (atom.range.end - range.end);
            let mut shifted = atom.clone();
            shifted.range = start..end;
            next_atoms.push(shifted);
        }
    }

    next_atoms.append(&mut inserted_atoms);
    next_atoms.sort_by_key(|atom| atom.range.start);
    next_atoms
}

pub(crate) fn ranges_intersect(left: &Range<usize>, right: &Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}
