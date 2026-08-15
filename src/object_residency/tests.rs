use gpui::{SharedString, px};

use super::*;
use crate::{
    BindingId, ByteOffset, InlineObjectId, InlineObjectOrder, InlineObjectPresentation,
    LogicalExtent, ObjectAnchorProofs, ObjectDemand, ObjectPageAdmissionError, ObjectPageEdgeFact,
    PageId, ScalarBoundaryProof, SourceRevision,
};

#[test]
fn duplicate_anchor_proof_rejection_preserves_pending_and_is_retryable() {
    let binding = RangeBinding::new(
        BindingId::new(7),
        SourceRevision::new(1),
        LogicalExtent::new(16, 1),
    );
    let limits = ObjectResidencyLimits::new(2, 4, 16 * 1024, 4096, 2, 4, 16 * 1024).unwrap();
    let mut residency = ObjectResidency::new(binding, PresentationGeneration::new(4), limits);
    let demand = ObjectDemandEnvelope::anchor(
        ByteOffset::new(4),
        None,
        crate::ObjectDirection::Forward,
        1,
        4096,
    )
    .unwrap();
    let ObjectDemand::Requested(request) = residency
        .demand(ObjectRequestId::new(1), ObjectPurpose::Viewport, demand)
        .unwrap()
    else {
        panic!("expected a new request")
    };
    let presentation = InlineObjectPresentation::new(
        1,
        SharedString::new_static("object"),
        px(20.),
        px(18.),
        px(14.),
        None,
        0,
        true,
    )
    .unwrap();
    let page = ObjectPage::new(
        ObjectPageId::new(1),
        request.key(),
        vec![InlineObjectFact::new(
            InlineObjectId::new(1),
            ByteOffset::new(4),
            InlineObjectOrder::new(10),
            "[1]",
            presentation,
        )],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proof = ScalarBoundaryProof::new(binding, ByteOffset::new(4), Some(PageId::new(9)));
    let before = format!("{residency:?}");
    let before_counts = residency.counts();
    let duplicate = ObjectAnchorProofs::new(binding, page.id(), page.key(), vec![proof, proof]);
    assert_eq!(
        residency.admit(page.clone(), duplicate),
        Err(ObjectPageAdmissionError::Malformed(
            ObjectContractError::ScalarBoundaryProofMismatch {
                anchor: ByteOffset::new(4)
            }
        ))
    );
    assert_eq!(format!("{residency:?}"), before);
    assert_eq!(residency.counts(), before_counts);
    let exact = ObjectAnchorProofs::new(binding, page.id(), page.key(), vec![proof]);
    assert!(residency.admit(page, exact).is_ok());
    assert_eq!(residency.counts().pending_requests, 0);
}
