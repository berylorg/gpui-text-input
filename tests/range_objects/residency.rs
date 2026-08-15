use super::*;

#[test]
fn residency_admits_same_anchor_pages_and_releases_exact_counts() {
    let text = text_residency("0123456789abcdef");
    let mut residency = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let demand = anchor_demand(5, None, ObjectDirection::Forward, 2, 4096);
    let request_key = requested_key(
        residency
            .demand(ObjectRequestId::new(1), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectRequestId::new(1),
    );
    assert_eq!(
        residency.counts(),
        ObjectResidencyCounts {
            pending_requests: 1,
            pending_objects: 2,
            pending_bytes: 4096,
            ..Default::default()
        }
    );
    let objects = vec![fact(1, 5, 10), fact(2, 5, 20)];
    let cursor = objects[1].cursor();
    let first = ObjectPage::new(
        ObjectPageId::new(1),
        request_key,
        objects,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::Continues(cursor),
        false,
        Some(cursor),
    )
    .unwrap();
    let first_charge = first.retained_charge();
    let proofs = anchor_proofs(&text, &first);
    assert_eq!(
        residency.admit(first, proofs),
        Ok(ObjectPageAdmission::Admitted {
            page: ObjectPageId::new(1),
            evicted_pages: 0,
            evicted_objects: 0,
        })
    );
    assert_eq!(residency.counts().resident_objects, 2);
    assert_eq!(residency.counts().resident_bytes, first_charge.bytes());
    assert_eq!(
        residency.counts().resident_presentation_bytes,
        first_charge.presentation_bytes()
    );

    let continued = anchor_demand(5, Some(cursor), ObjectDirection::Forward, 2, 4096);
    let second_key = requested_key(
        residency
            .demand(ObjectRequestId::new(2), ObjectPurpose::Viewport, continued)
            .unwrap(),
        ObjectRequestId::new(2),
    );
    let second = ObjectPage::new(
        ObjectPageId::new(2),
        second_key,
        vec![fact(3, 5, 30)],
        ObjectPageEdgeFact::Continues(cursor),
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = anchor_proofs(&text, &second);
    residency.admit(second, proofs).unwrap();
    assert_eq!(residency.counts().resident_pages, 2);
    assert_eq!(residency.counts().resident_objects, 3);
    assert!(residency.evict(ObjectPageId::new(1)));
    assert_eq!(residency.counts().resident_objects, 1);
    let pages = residency.take_resident_pages();
    assert_eq!(pages.len(), 1);
    assert_eq!(residency.counts(), ObjectResidencyCounts::default());
}

#[test]
fn cancellation_stale_generation_rebind_and_dispose_release_reservations() {
    let mut residency = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let demand = anchor_demand(0, None, ObjectDirection::Forward, 1, 1024);
    let first = requested_key(
        residency
            .demand(ObjectRequestId::new(1), ObjectPurpose::Caret, demand)
            .unwrap(),
        ObjectRequestId::new(1),
    );
    assert_eq!(
        residency.cancel(first),
        ObjectPageSettlement::Settled(ObjectPageFailure::Cancelled)
    );
    assert_eq!(
        residency.cancel(first),
        ObjectPageSettlement::AlreadyCancelled
    );
    assert_eq!(residency.counts(), ObjectResidencyCounts::default());

    let second = requested_key(
        residency
            .demand(ObjectRequestId::new(2), ObjectPurpose::Caret, demand)
            .unwrap(),
        ObjectRequestId::new(2),
    );
    let cancelled = residency.rebind(binding(1), PresentationGeneration::new(5));
    assert_eq!(cancelled, vec![second]);
    assert_eq!(residency.counts(), ObjectResidencyCounts::default());
    assert_eq!(
        residency.settle(second, ObjectPageFailure::Unavailable),
        ObjectPageSettlement::Stale
    );

    let third = requested_key(
        residency
            .demand(ObjectRequestId::new(1), ObjectPurpose::Caret, demand)
            .unwrap(),
        ObjectRequestId::new(1),
    );
    assert_eq!(residency.dispose(), vec![third]);
    assert_eq!(residency.counts(), ObjectResidencyCounts::default());
}

#[test]
fn stale_binding_revision_and_exact_request_identity_never_admit() {
    let text = text_residency("0123456789abcdef");
    let mut residency = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let demand = anchor_demand(16, None, ObjectDirection::Forward, 1, 4096);
    let admitted_key = requested_key(
        residency
            .demand(ObjectRequestId::new(1), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectRequestId::new(1),
    );
    let wrong_key = ObjectRequestKey::new(
        ObjectRequestId::new(2),
        BindingId::new(7),
        SourceRevision::new(2),
        PresentationGeneration::new(4),
        ObjectPurpose::Viewport,
        demand,
    )
    .unwrap();
    let stale = ObjectPage::new(
        ObjectPageId::new(9),
        wrong_key,
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let current_proof_page = ObjectPage::new(
        ObjectPageId::new(9),
        admitted_key,
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let stale_proofs = text
        .prove_object_page_anchors(residency.binding(), &current_proof_page)
        .unwrap();
    assert_eq!(
        residency.admit(stale, stale_proofs),
        Err(ObjectPageAdmissionError::Stale(wrong_key))
    );
    assert_eq!(residency.counts().pending_requests, 1);

    let valid = ObjectPage::new(
        ObjectPageId::new(10),
        admitted_key,
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = text
        .prove_object_page_anchors(residency.binding(), &valid)
        .unwrap();
    residency.admit(valid, proofs).unwrap();
    assert_eq!(residency.counts().pending_requests, 0);
}

#[test]
fn pending_count_object_and_byte_caps_are_independent_and_exact() {
    let limits = ObjectResidencyLimits::new(1, 2, 8192, 4096, 1, 2, 2048).unwrap();
    let mut residency = ObjectResidency::new(binding(1), PresentationGeneration::new(4), limits);
    let too_many = anchor_demand(5, None, ObjectDirection::Forward, 3, 1024);
    assert_eq!(
        residency.demand(ObjectRequestId::new(1), ObjectPurpose::Viewport, too_many),
        Err(gpui_text_input::ObjectDemandError::LimitExceeded(
            ObjectResidencyLimitKind::PendingObjects
        ))
    );
    let too_large = anchor_demand(5, None, ObjectDirection::Forward, 2, 2049);
    assert_eq!(
        residency.demand(ObjectRequestId::new(2), ObjectPurpose::Viewport, too_large),
        Err(gpui_text_input::ObjectDemandError::LimitExceeded(
            ObjectResidencyLimitKind::PendingBytes
        ))
    );
    let admitted = anchor_demand(5, None, ObjectDirection::Forward, 2, 2048);
    let request = requested_key(
        residency
            .demand(ObjectRequestId::new(3), ObjectPurpose::Viewport, admitted)
            .unwrap(),
        ObjectRequestId::new(3),
    );
    assert_eq!(residency.counts().pending_objects, 2);
    assert_eq!(residency.counts().pending_bytes, 2048);
    assert_eq!(
        residency.settle(request, ObjectPageFailure::Unavailable),
        ObjectPageSettlement::Settled(ObjectPageFailure::Unavailable)
    );
    assert_eq!(residency.counts(), ObjectResidencyCounts::default());
}

#[test]
fn demand_extent_and_cursor_anchor_are_validated_without_guessing_scalar_boundaries() {
    let mut residency = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let outside = anchor_demand(17, None, ObjectDirection::Forward, 1, 1024);
    assert_eq!(
        residency.demand(ObjectRequestId::new(1), ObjectPurpose::Restoration, outside),
        Err(gpui_text_input::ObjectDemandError::Malformed(
            ObjectContractError::DemandOutsideExtent
        ))
    );
    assert_eq!(
        ObjectDemandEnvelope::anchor(
            ByteOffset::new(5),
            Some(ObjectCursor::new(
                ByteOffset::new(6),
                InlineObjectOrder::new(1),
                InlineObjectId::new(1),
            )),
            ObjectDirection::Forward,
            1,
            1024,
        ),
        Err(ObjectContractError::CursorOutsideEnvelope)
    );
}

#[test]
fn resident_page_cap_evicts_lru_without_growing_object_or_byte_counts() {
    let text = text_residency("0123456789abcdef");
    let limits = ObjectResidencyLimits::new(1, 2, 8192, 4096, 2, 2, 8192).unwrap();
    let mut residency = ObjectResidency::new(binding(1), PresentationGeneration::new(4), limits);
    for (request_id, anchor, object_id) in [(1, 4, 1), (2, 8, 2)] {
        let demand = anchor_demand(anchor, None, ObjectDirection::Forward, 1, 4096);
        let request = requested_key(
            residency
                .demand(
                    ObjectRequestId::new(request_id),
                    ObjectPurpose::Viewport,
                    demand,
                )
                .unwrap(),
            ObjectRequestId::new(request_id),
        );
        let page = ObjectPage::new(
            ObjectPageId::new(request_id),
            request,
            vec![fact(object_id, anchor, 10)],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap();
        let proofs = anchor_proofs(&text, &page);
        let admission = residency.admit(page, proofs).unwrap();
        assert_eq!(residency.counts().resident_pages, 1);
        assert_eq!(residency.counts().resident_objects, 1);
        if request_id == 2 {
            assert_eq!(
                admission,
                ObjectPageAdmission::Admitted {
                    page: ObjectPageId::new(2),
                    evicted_pages: 1,
                    evicted_objects: 1,
                }
            );
        }
    }
}

#[test]
fn resident_identity_and_order_conflicts_preserve_and_retry_the_new_request() {
    let text = text_residency("0123456789abcdef");
    let mut residency = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let demand = anchor_demand(5, None, ObjectDirection::Forward, 1, 4096);
    let first_key = requested_key(
        residency
            .demand(ObjectRequestId::new(1), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectRequestId::new(1),
    );
    let first = ObjectPage::new(
        ObjectPageId::new(1),
        first_key,
        vec![fact(1, 5, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = anchor_proofs(&text, &first);
    residency.admit(first, proofs).unwrap();

    let conflicting_key = requested_key(
        residency
            .demand(ObjectRequestId::new(2), ObjectPurpose::Caret, demand)
            .unwrap(),
        ObjectRequestId::new(2),
    );
    let conflicting = ObjectPage::new(
        ObjectPageId::new(2),
        conflicting_key,
        vec![fact(2, 5, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = anchor_proofs(&text, &conflicting);
    let prior_fingerprint = format!("{residency:?}");
    let prior_counts = residency.counts();
    assert!(matches!(
        residency.admit(conflicting.clone(), proofs),
        Err(ObjectPageAdmissionError::Malformed(
            ObjectContractError::DuplicateObjectOrder { .. }
        ))
    ));
    assert_eq!(format!("{residency:?}"), prior_fingerprint);
    assert_eq!(residency.counts(), prior_counts);

    let retry = ObjectPage::new(
        ObjectPageId::new(20),
        conflicting.key(),
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let retry_proofs = anchor_proofs(&text, &retry);
    residency.admit(retry, retry_proofs).unwrap();
    assert_eq!(residency.counts().pending_requests, 0);
    assert_eq!(residency.counts().resident_objects, 1);

    let moved_demand = anchor_demand(6, None, ObjectDirection::Forward, 1, 4096);
    let moved_key = requested_key(
        residency
            .demand(ObjectRequestId::new(3), ObjectPurpose::Caret, moved_demand)
            .unwrap(),
        ObjectRequestId::new(3),
    );
    let moved = ObjectPage::new(
        ObjectPageId::new(3),
        moved_key,
        vec![fact(1, 6, 20)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = anchor_proofs(&text, &moved);
    let prior_fingerprint = format!("{residency:?}");
    let prior_counts = residency.counts();
    assert!(matches!(
        residency.admit(moved.clone(), proofs),
        Err(ObjectPageAdmissionError::Malformed(
            ObjectContractError::ConflictingObjectIdentity { .. }
        ))
    ));
    assert_eq!(format!("{residency:?}"), prior_fingerprint);
    assert_eq!(residency.counts(), prior_counts);

    let retry = ObjectPage::new(
        ObjectPageId::new(30),
        moved.key(),
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let retry_proofs = anchor_proofs(&text, &retry);
    residency.admit(retry, retry_proofs).unwrap();
    assert_eq!(residency.counts().pending_requests, 0);
}
