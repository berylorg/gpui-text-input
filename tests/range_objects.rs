use gpui::{SharedString, StreamingLayoutPosition, px};
use gpui_text_input::{
    BindingId, ByteOffset, ByteRange, InlineObjectFact, InlineObjectGap, InlineObjectGapError,
    InlineObjectId, InlineObjectNeighbor, InlineObjectOrder, InlineObjectPresentation,
    LogicalExtent, ObjectAnchorProofError, ObjectAnchorProofs, ObjectContractError, ObjectCursor,
    ObjectDemand, ObjectDemandEnvelope, ObjectDirection, ObjectPage, ObjectPageAdmission,
    ObjectPageAdmissionError, ObjectPageCharge, ObjectPageEdgeFact, ObjectPageFailure,
    ObjectPageId, ObjectPageSettlement, ObjectPurpose, ObjectRequestId, ObjectRequestKey,
    ObjectResidency, ObjectResidencyCounts, ObjectResidencyLimitKind, ObjectResidencyLimits,
    PageDemand, PageDemandEnvelope, PageDirection, PageEdgeFact, PageId, PagePurpose,
    PageRequestId, PresentationGeneration, RangeBinding, RangePage, RangeResidency,
    ResidencyLimits, ScalarBoundaryProofError, SourcePosition, SourceRange, SourceRangeError,
    SourceRevision,
};

fn binding(revision: u64) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(7),
        SourceRevision::new(revision),
        LogicalExtent::new(16, 1),
    )
}

fn presentation(key: u64, display: &'static str) -> InlineObjectPresentation {
    InlineObjectPresentation::new(
        key,
        SharedString::new_static(display),
        px(20.),
        px(18.),
        px(14.),
        None,
        3,
        true,
    )
    .unwrap()
}

fn fact(id: u128, anchor: u64, order: u128) -> InlineObjectFact {
    InlineObjectFact::new(
        InlineObjectId::new(id),
        ByteOffset::new(anchor),
        InlineObjectOrder::new(order),
        format!("[{id}]"),
        presentation(id as u64, "object"),
    )
}

fn anchor_demand(
    anchor: u64,
    cursor: Option<ObjectCursor>,
    direction: ObjectDirection,
    max_objects: usize,
    max_bytes: usize,
) -> ObjectDemandEnvelope {
    ObjectDemandEnvelope::anchor(
        ByteOffset::new(anchor),
        cursor,
        direction,
        max_objects,
        max_bytes,
    )
    .unwrap()
}

fn key(id: u64, demand: ObjectDemandEnvelope) -> ObjectRequestKey {
    ObjectRequestKey::new(
        ObjectRequestId::new(id),
        BindingId::new(7),
        SourceRevision::new(1),
        PresentationGeneration::new(4),
        ObjectPurpose::Viewport,
        demand,
    )
    .unwrap()
}

fn residency_limits() -> ObjectResidencyLimits {
    ObjectResidencyLimits::new(2, 4, 16 * 1024, 4096, 2, 4, 16 * 1024).unwrap()
}

fn requested_key(demand: ObjectDemand, expected: ObjectRequestId) -> ObjectRequestKey {
    let ObjectDemand::Requested(request) = demand else {
        panic!("expected a new request")
    };
    assert_eq!(request.key().id(), expected);
    request.key()
}

fn text_residency(source: &str) -> RangeResidency {
    assert_eq!(source.len(), 16);
    let mut residency = RangeResidency::new(
        binding(1),
        ResidencyLimits::new(2, 16 * 1024, 2, 32).unwrap(),
    );
    let demand = PageDemandEnvelope::Adjacent {
        anchor: ByteOffset::new(0),
        direction: PageDirection::Forward,
        max_payload_bytes: 16,
    };
    let PageDemand::Requested(request) = residency
        .demand(PageRequestId::new(1), PagePurpose::Viewport, demand)
        .unwrap()
    else {
        panic!("expected text request")
    };
    let page = RangePage::new(
        PageId::new(1),
        request.key(),
        ByteRange::from_u64(0, 16).unwrap(),
        source.to_owned(),
        vec![],
        PageEdgeFact::DocumentBoundary,
        PageEdgeFact::DocumentBoundary,
        true,
    )
    .unwrap();
    residency.admit(page).unwrap();
    residency
}

fn anchor_proofs(text: &RangeResidency, page: &ObjectPage) -> ObjectAnchorProofs {
    text.prove_object_page_anchors(binding(1), page).unwrap()
}

#[path = "range_objects/caps.rs"]
mod caps;
#[path = "range_objects/residency.rs"]
mod residency;
#[path = "range_objects/review.rs"]
mod review;

#[test]
fn composite_positions_cover_empty_before_between_after_and_gpui_round_trip() {
    let anchor = ByteOffset::new(8);
    let first = InlineObjectNeighbor::new(InlineObjectId::new(10), InlineObjectOrder::new(20));
    let second = InlineObjectNeighbor::new(InlineObjectId::new(11), InlineObjectOrder::new(30));
    let empty = SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects);
    let before = SourcePosition::new(anchor, InlineObjectGap::before(first));
    let between = SourcePosition::new(anchor, InlineObjectGap::between(first, second).unwrap());
    let after = SourcePosition::new(anchor, InlineObjectGap::after(second));

    assert!(before.compare_in_revision(between).unwrap().is_lt());
    assert!(between.compare_in_revision(after).unwrap().is_lt());
    assert!(SourceRange::new(before, after).is_ok());
    assert_eq!(
        SourcePosition::try_from(StreamingLayoutPosition::from(empty)).unwrap(),
        empty
    );
    assert_eq!(
        SourcePosition::try_from(StreamingLayoutPosition::from(before)).unwrap(),
        before
    );
    assert_eq!(
        SourcePosition::try_from(StreamingLayoutPosition::from(between)).unwrap(),
        between
    );
    assert_eq!(
        SourcePosition::try_from(StreamingLayoutPosition::from(after)).unwrap(),
        after
    );
    assert!(std::mem::size_of::<SourcePosition>() <= 128);
}

#[test]
fn malformed_gap_and_incompatible_same_anchor_range_are_rejected() {
    let first = InlineObjectNeighbor::new(InlineObjectId::new(1), InlineObjectOrder::new(4));
    let duplicate = InlineObjectGap::between(first, first);
    assert_eq!(
        duplicate,
        Err(InlineObjectGapError::DuplicateIdentity(
            InlineObjectId::new(1)
        ))
    );

    let no_objects = SourcePosition::new(ByteOffset::new(3), InlineObjectGap::NoObjects);
    let before = SourcePosition::new(ByteOffset::new(3), InlineObjectGap::before(first));
    assert!(matches!(
        SourceRange::new(no_objects, before),
        Err(SourceRangeError::IncompatibleGapWitnesses { .. })
    ));
}

#[test]
fn lib_docs_object_source_example_is_covered_by_nextest() {
    let text = RangeResidency::new(
        binding(1),
        ResidencyLimits::new(2, 16 * 1024, 2, 32).unwrap(),
    );
    let mut objects = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let demand = anchor_demand(0, None, ObjectDirection::Forward, 4, 4096);
    let request = requested_key(
        objects
            .demand(ObjectRequestId::new(1), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectRequestId::new(1),
    );
    let id = InlineObjectId::new(9);
    let order = InlineObjectOrder::new(20);
    let page = ObjectPage::new(
        ObjectPageId::new(1),
        request,
        vec![InlineObjectFact::new(
            id,
            ByteOffset::new(0),
            order,
            "attachment",
            presentation(5, "[object]"),
        )],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = text
        .prove_object_page_anchors(objects.binding(), &page)
        .unwrap();
    objects.admit(page, proofs).unwrap();
    let before = SourcePosition::new(
        ByteOffset::new(0),
        InlineObjectGap::before(InlineObjectNeighbor::new(id, order)),
    );
    assert_eq!(StreamingLayoutPosition::from(before).byte_offset, 0);
    assert_eq!(objects.counts().resident_objects, 1);
}

#[test]
fn empty_anchor_page_is_complete_at_origin_and_source_end() {
    for anchor in [0, 16] {
        let demand = anchor_demand(anchor, None, ObjectDirection::Forward, 2, 4096);
        let page = ObjectPage::new(
            ObjectPageId::new(anchor + 1),
            key(anchor + 1, demand),
            vec![],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap();
        assert!(page.objects().is_empty());
        assert!(page.complete());
        assert_eq!(page.continuation(), None);
    }
}

#[test]
fn same_anchor_run_pages_with_cursor_only_progress() {
    let demand = anchor_demand(5, None, ObjectDirection::Forward, 2, 4096);
    let first_objects = vec![fact(1, 5, 10), fact(2, 5, 20)];
    let cursor = first_objects[1].cursor();
    let first = ObjectPage::new(
        ObjectPageId::new(1),
        key(1, demand),
        first_objects,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::Continues(cursor),
        false,
        Some(cursor),
    )
    .unwrap();
    assert_eq!(first.continuation(), Some(cursor));

    let continued = anchor_demand(5, Some(cursor), ObjectDirection::Forward, 2, 4096);
    let second = ObjectPage::new(
        ObjectPageId::new(2),
        key(2, continued),
        vec![fact(3, 5, 30)],
        ObjectPageEdgeFact::Continues(cursor),
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    assert_eq!(second.objects()[0].anchor(), ByteOffset::new(5));
    assert!(second.complete());
}

#[test]
fn backward_same_anchor_pages_remain_source_ordered_and_resume_from_first() {
    let demand = anchor_demand(5, None, ObjectDirection::Backward, 2, 4096);
    let objects = vec![fact(2, 5, 20), fact(3, 5, 30)];
    let cursor = objects[0].cursor();
    let first = ObjectPage::new(
        ObjectPageId::new(11),
        key(11, demand),
        objects,
        ObjectPageEdgeFact::Continues(cursor),
        ObjectPageEdgeFact::EnvelopeBoundary,
        false,
        Some(cursor),
    )
    .unwrap();
    assert_eq!(first.continuation(), Some(cursor));

    let continued = anchor_demand(5, Some(cursor), ObjectDirection::Backward, 2, 4096);
    let second = ObjectPage::new(
        ObjectPageId::new(12),
        key(12, continued),
        vec![fact(1, 5, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::Continues(cursor),
        true,
        None,
    )
    .unwrap();
    assert_eq!(second.objects()[0].order(), InlineObjectOrder::new(10));
}

#[test]
fn range_pages_allow_both_edges_and_preserve_strict_tuple_order() {
    let range = ByteRange::from_u64(4, 8).unwrap();
    let demand =
        ObjectDemandEnvelope::range(range, None, ObjectDirection::Forward, 4, 8192).unwrap();
    let page = ObjectPage::new(
        ObjectPageId::new(3),
        key(3, demand),
        vec![fact(1, 4, 10), fact(2, 6, 10), fact(3, 8, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    assert_eq!(page.objects().len(), 3);
}

#[test]
fn duplicate_identity_order_and_malformed_continuation_are_rejected() {
    let demand = anchor_demand(5, None, ObjectDirection::Forward, 4, 8192);
    assert!(matches!(
        ObjectPage::new(
            ObjectPageId::new(4),
            key(4, demand),
            vec![fact(1, 5, 10), fact(1, 5, 20)],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        ),
        Err(ObjectContractError::DuplicateObjectIdentity { .. })
    ));
    assert!(matches!(
        ObjectPage::new(
            ObjectPageId::new(5),
            key(5, demand),
            vec![fact(1, 5, 10), fact(2, 5, 10)],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        ),
        Err(ObjectContractError::DuplicateObjectOrder { .. })
    ));
    assert_eq!(
        ObjectPage::new(
            ObjectPageId::new(6),
            key(6, demand),
            vec![],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            false,
            None,
        ),
        Err(ObjectContractError::NonProgressingObjectPage)
    );
}

#[test]
fn continuation_rejects_duplicate_cursor_identity_and_same_anchor_order() {
    let cursor = ObjectCursor::new(
        ByteOffset::new(5),
        InlineObjectOrder::new(10),
        InlineObjectId::new(1),
    );
    let demand = anchor_demand(5, Some(cursor), ObjectDirection::Forward, 1, 4096);
    for (object, expected_identity) in [(fact(1, 5, 20), true), (fact(2, 5, 10), false)] {
        let error = ObjectPage::new(
            ObjectPageId::new(20),
            key(20, demand),
            vec![object],
            ObjectPageEdgeFact::Continues(cursor),
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap_err();
        assert_eq!(
            matches!(&error, ObjectContractError::DuplicateObjectIdentity { .. }),
            expected_identity
        );
        assert_eq!(
            matches!(&error, ObjectContractError::DuplicateObjectOrder { .. }),
            !expected_identity
        );
    }
}

#[test]
fn page_caps_and_presentation_metrics_are_enforced_before_retention() {
    assert!(matches!(
        InlineObjectPresentation::new(
            1,
            SharedString::new_static("bad"),
            px(0.),
            px(10.),
            px(5.),
            None,
            0,
            false,
        ),
        Err(ObjectContractError::InvalidPresentationMetrics)
    ));
    let count_limited = anchor_demand(5, None, ObjectDirection::Forward, 1, 8192);
    assert!(matches!(
        ObjectPage::new(
            ObjectPageId::new(7),
            key(7, count_limited),
            vec![fact(1, 5, 10), fact(2, 5, 20)],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        ),
        Err(ObjectContractError::ObjectCountLimitExceeded)
    ));
    let byte_limited = anchor_demand(5, None, ObjectDirection::Forward, 1, 1);
    assert!(matches!(
        ObjectPage::new(
            ObjectPageId::new(8),
            key(8, byte_limited),
            vec![fact(1, 5, 10)],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        ),
        Err(ObjectContractError::RetainedByteLimitExceeded)
    ));

    let charged = ObjectPage::new(
        ObjectPageId::new(9),
        key(9, anchor_demand(5, None, ObjectDirection::Forward, 1, 8192)),
        vec![fact(1, 5, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    assert_eq!(
        charged.retained_charge().presentation_bytes(),
        "object".len() + "[1]".len(),
        "presentation accounting retains only display and clipboard fallback bytes"
    );
}
