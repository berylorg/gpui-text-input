use super::ByteRange;
use std::mem::size_of;

use super::{
    AtomFact, PageDemandEnvelope, PageDirection, PageEdgeFact, PageId, PageRequestKey,
    RangeContractError, RangePage, RangePageCharge,
};

impl RangePage {
    /// Constructs and validates one source-selected exact page payload.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: PageId,
        key: PageRequestKey,
        range: ByteRange,
        text: String,
        atoms: Vec<AtomFact>,
        preceding: PageEdgeFact,
        following: PageEdgeFact,
        end_of_source: bool,
    ) -> Result<Self, RangeContractError> {
        if u64::try_from(text.len()).ok() != Some(range.len()) {
            return Err(RangeContractError::PayloadLengthMismatch {
                range,
                actual_bytes: text.len(),
            });
        }
        if (range.start().get() == 0) != (preceding == PageEdgeFact::DocumentBoundary) {
            return Err(RangeContractError::MalformedEdgeFacts);
        }
        if end_of_source != (following == PageEdgeFact::DocumentBoundary) {
            return Err(RangeContractError::MalformedEdgeFacts);
        }
        validate_envelope(key, range, preceding, following)?;

        let mut previous_end = range.start();
        let mut retained_bytes = text.len();
        for (index, atom) in atoms.iter().enumerate() {
            if atoms[..index].iter().any(|prior| prior.id() == atom.id()) {
                return Err(RangeContractError::DuplicateAtomFact { atom: atom.id() });
            }
            let malformed = || RangeContractError::MalformedAtomRange {
                atom: atom.id(),
                global_range: atom.global_range(),
                fragment_range: atom.fragment_range(),
            };
            if atom.global_range().is_empty()
                || atom.global_range().intersection(range) != Some(atom.fragment_range())
                || atom.fragment_range().start() < previous_end
            {
                return Err(malformed());
            }
            let local_start =
                usize::try_from(atom.fragment_range().start().get() - range.start().get())
                    .map_err(|_| malformed())?;
            let local_end =
                usize::try_from(atom.fragment_range().end().get() - range.start().get())
                    .map_err(|_| malformed())?;
            if !text.is_char_boundary(local_start) || !text.is_char_boundary(local_end) {
                return Err(malformed());
            }
            previous_end = atom.fragment_range().end();
            retained_bytes = retained_bytes
                .checked_add(atom.retained_bytes())
                .ok_or(RangeContractError::PayloadByteCountOverflow)?;
        }
        let retained_charge = RangePageCharge {
            bytes: size_of::<RangePage>()
                .checked_add(
                    atoms
                        .len()
                        .checked_mul(size_of::<AtomFact>())
                        .ok_or(RangeContractError::PayloadByteCountOverflow)?,
                )
                .and_then(|bytes| bytes.checked_add(retained_bytes))
                .ok_or(RangeContractError::PayloadByteCountOverflow)?,
            items: atoms
                .len()
                .checked_add(1)
                .ok_or(RangeContractError::PayloadByteCountOverflow)?,
        };

        Ok(Self {
            id,
            key,
            range,
            text,
            atoms,
            preceding,
            following,
            end_of_source,
            retained_bytes,
            retained_charge,
        })
    }

    /// Returns the stable page payload identity.
    pub const fn id(&self) -> PageId {
        self.id
    }

    /// Returns the exact request/response key.
    pub const fn key(&self) -> PageRequestKey {
        self.key
    }

    /// Returns the exact absolute page range.
    pub const fn range(&self) -> ByteRange {
        self.range
    }

    /// Returns this bounded page's valid UTF-8 payload.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns authoritative atom facts in display order.
    pub fn atoms(&self) -> &[AtomFact] {
        &self.atoms
    }

    /// Returns the authoritative leading-edge fact.
    pub const fn preceding(&self) -> PageEdgeFact {
        self.preceding
    }

    /// Returns the authoritative trailing-edge fact.
    pub const fn following(&self) -> PageEdgeFact {
        self.following
    }

    /// Reports whether the page ends at the logical source end.
    pub const fn end_of_source(&self) -> bool {
        self.end_of_source
    }

    /// Returns exactly retained UTF-8 page and fallback-copy bytes.
    pub const fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }

    /// Returns the complete borrowed page-record charge used for exact admission.
    pub const fn retained_charge(&self) -> RangePageCharge {
        self.retained_charge
    }

    /// Clones one bounded resident payload under another exact equivalent demand.
    pub(crate) fn clone_for_request(
        &self,
        key: PageRequestKey,
    ) -> Result<Self, RangeContractError> {
        Self::new(
            self.id,
            key,
            self.range,
            self.text.clone(),
            self.atoms.clone(),
            self.preceding,
            self.following,
            self.end_of_source,
        )
    }

    /// Returns the validation result when this page answers a validation demand.
    ///
    /// `None` identifies an adjacent demand. `Some(false)` rejects the candidate exactly; it is
    /// never rounded or translated to a nearby UTF-8 boundary.
    pub fn candidate_is_boundary(&self) -> Option<bool> {
        let PageDemandEnvelope::Validation { candidate, .. } = self.key.demand() else {
            return None;
        };
        let local = usize::try_from(candidate.get() - self.range.start().get()).ok()?;
        Some(self.text.is_char_boundary(local))
    }
}

fn validate_envelope(
    key: PageRequestKey,
    range: ByteRange,
    preceding: PageEdgeFact,
    following: PageEdgeFact,
) -> Result<(), RangeContractError> {
    let demand = key.demand();
    if range.len() > demand.max_payload_bytes() {
        return Err(RangeContractError::ReturnedRangeOutsideEnvelope {
            demand,
            returned: range,
        });
    }
    match demand {
        PageDemandEnvelope::Adjacent {
            anchor, direction, ..
        } => {
            let anchored = match direction {
                PageDirection::Forward => range.start() == anchor,
                PageDirection::Backward => range.end() == anchor,
            };
            if !anchored {
                return Err(RangeContractError::ReturnedRangeOutsideEnvelope {
                    demand,
                    returned: range,
                });
            }
            if range.is_empty() {
                let matching_document_edge = match direction {
                    PageDirection::Forward => following == PageEdgeFact::DocumentBoundary,
                    PageDirection::Backward => preceding == PageEdgeFact::DocumentBoundary,
                };
                if !matching_document_edge {
                    return Err(RangeContractError::NonProgressingPage { anchor, direction });
                }
            }
        }
        PageDemandEnvelope::Validation { candidate, .. } => {
            if !range.contains_offset(candidate)
                || (range.is_empty()
                    && preceding != PageEdgeFact::DocumentBoundary
                    && following != PageEdgeFact::DocumentBoundary)
            {
                return Err(RangeContractError::ReturnedRangeOutsideEnvelope {
                    demand,
                    returned: range,
                });
            }
        }
    }
    Ok(())
}
