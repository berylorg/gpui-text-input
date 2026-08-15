use gpui_text_input::{
    BindingId, ByteOffset, ByteRange, LogicalExtent, MutationFragment, MutationFragmentPayload,
    MutationKey, MutationKind, MutationLimits, MutationOutcome, MutationPositions,
    MutationProposal, ObjectDemand, ObjectDemandEnvelope, ObjectDirection, ObjectPage,
    ObjectPageEdgeFact, ObjectPageId, ObjectPurpose, ObjectRequestId, ObjectResidency,
    ObjectResidencyLimits, OperationId, PageDemand, PageDemandEnvelope, PageDirection,
    PageEdgeFact, PageId, PagePurpose, PageRequestId, PresentationGeneration, RangeBinding,
    RangeEditCoordinator, RangePage, RangeResidency, ResidencyLimits, SourcePosition, SourceRange,
    SourceRevision,
};

fn binding(revision: u64, text: &str) -> RangeBinding {
    let lines = if text.is_empty() {
        0
    } else {
        text.bytes().filter(|byte| *byte == b'\n').count() as u64 + 1
    };
    RangeBinding::new(
        BindingId::new(7),
        SourceRevision::new(revision),
        LogicalExtent::new(text.len() as u64, lines),
    )
}

fn position(offset: u64) -> SourcePosition {
    SourcePosition::new(
        ByteOffset::new(offset),
        gpui_text_input::InlineObjectGap::NoObjects,
    )
}

fn source_range(start: u64, end: u64) -> SourceRange {
    SourceRange::new(position(start), position(end)).unwrap()
}

fn key(binding: RangeBinding, operation: u64) -> MutationKey {
    MutationKey::new(
        binding.binding(),
        binding.revision(),
        OperationId::new(operation),
    )
}

fn proposal(
    binding: RangeBinding,
    operation: u64,
    range: SourceRange,
    removed_breaks: u64,
) -> MutationProposal {
    MutationProposal::new(
        key(binding, operation),
        MutationKind::Edit,
        range,
        removed_breaks,
    )
}

fn terminal(key: MutationKey, ordinal: usize, position: SourcePosition) -> MutationFragment {
    MutationFragment::new(
        key,
        ordinal,
        MutationFragmentPayload::Terminal {
            intended: MutationPositions::collapsed(position),
        },
    )
}

fn admitted_sources(
    binding: RangeBinding,
    text: &str,
    at: SourcePosition,
    facts: Vec<gpui_text_input::InlineObjectFact>,
    nonce: u64,
) -> (RangeResidency, ObjectResidency) {
    admitted_sources_for_positions(binding, text, &[at], facts, nonce)
}

fn admitted_sources_for_positions(
    binding: RangeBinding,
    text: &str,
    positions: &[SourcePosition],
    facts: Vec<gpui_text_input::InlineObjectFact>,
    nonce: u64,
) -> (RangeResidency, ObjectResidency) {
    let mut text_residency = RangeResidency::new(
        binding,
        ResidencyLimits::new(4, 64 * 1024, 4, 64 * 1024).unwrap(),
    );
    let PageDemand::Requested(text_request) = text_residency
        .demand(
            PageRequestId::new(nonce),
            PagePurpose::Caret,
            PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(0),
                direction: PageDirection::Forward,
                max_payload_bytes: (text.len() as u64).max(4),
            },
        )
        .unwrap()
    else {
        panic!("expected text request")
    };
    let text_page = RangePage::new(
        PageId::new(nonce),
        text_request.key(),
        ByteRange::from_u64(0, text.len() as u64).unwrap(),
        text.to_owned(),
        vec![],
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap();
    text_residency.admit(text_page).unwrap();
    let mut object_residency = ObjectResidency::new(
        binding,
        PresentationGeneration::new(1),
        ObjectResidencyLimits::new(8, 16, 64 * 1024, 32 * 1024, 8, 16, 64 * 1024).unwrap(),
    );
    let mut anchors = Vec::new();
    for position in positions {
        if anchors.contains(&position.byte_offset) {
            continue;
        }
        anchors.push(position.byte_offset);
        let anchor_facts = facts
            .iter()
            .filter(|fact| fact.anchor() == position.byte_offset)
            .cloned()
            .collect::<Vec<_>>();
        let demand = ObjectDemandEnvelope::anchor(
            position.byte_offset,
            None,
            ObjectDirection::Forward,
            anchor_facts.len().max(1),
            4096,
        )
        .unwrap();
        let page_nonce = nonce + anchors.len() as u64;
        let ObjectDemand::Requested(object_request) = object_residency
            .demand(
                ObjectRequestId::new(page_nonce),
                ObjectPurpose::MutationSuccessor,
                demand,
            )
            .unwrap()
        else {
            panic!("expected object request")
        };
        let object_page = ObjectPage::new(
            ObjectPageId::new(page_nonce),
            object_request.key(),
            anchor_facts,
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap();
        let proofs = text_residency
            .prove_object_page_anchors(binding, &object_page)
            .unwrap();
        object_residency.admit(object_page, proofs).unwrap();
    }
    (text_residency, object_residency)
}

fn editor(binding: RangeBinding) -> RangeEditCoordinator {
    RangeEditCoordinator::new(
        binding,
        MutationLimits::new(16, 4096)
            .unwrap()
            .with_object_limits(8, 1024, 512)
            .unwrap(),
    )
}

#[path = "range_mutations/lifecycle.rs"]
mod lifecycle;
#[path = "range_mutations/operations.rs"]
mod operations;
