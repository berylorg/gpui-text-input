use super::*;

fn page_for(
    residency: &mut ObjectResidency,
    request_id: u64,
    page_id: u64,
    purpose: ObjectPurpose,
    anchor: u64,
    object_id: u128,
) -> ObjectPage {
    let demand = anchor_demand(anchor, None, ObjectDirection::Forward, 1, 4096);
    let request = requested_key(
        residency
            .demand(ObjectRequestId::new(request_id), purpose, demand)
            .unwrap(),
        ObjectRequestId::new(request_id),
    );
    ObjectPage::new(
        ObjectPageId::new(page_id),
        request,
        vec![fact(object_id, anchor, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap()
}

#[test]
fn object_admission_requires_matching_text_owned_scalar_proofs() {
    let text = text_residency("é234567890abcde");
    assert_eq!(
        text.prove_scalar_boundary(ByteOffset::new(1)),
        Err(ScalarBoundaryProofError::NotScalarBoundary(
            ByteOffset::new(1)
        ))
    );

    let empty_text = RangeResidency::new(binding(1), ResidencyLimits::new(1, 4096, 1, 16).unwrap());
    assert_eq!(
        empty_text.prove_scalar_boundary(ByteOffset::new(5)),
        Err(ScalarBoundaryProofError::Unavailable(ByteOffset::new(5)))
    );

    let mut objects = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let page = page_for(&mut objects, 1, 1, ObjectPurpose::Viewport, 1, 1);
    assert_eq!(
        text.prove_object_page_anchors(objects.binding(), &page),
        Err(ObjectAnchorProofError::Scalar(
            ScalarBoundaryProofError::NotScalarBoundary(ByteOffset::new(1))
        ))
    );
    let proof_demand = anchor_demand(0, None, ObjectDirection::Forward, 1, 4096);
    let proof_page = ObjectPage::new(
        ObjectPageId::new(99),
        key(99, proof_demand),
        vec![fact(99, 0, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let wrong_offset = text
        .prove_object_page_anchors(objects.binding(), &proof_page)
        .unwrap();
    let prior_fingerprint = format!("{objects:?}");
    let prior_counts = objects.counts();
    assert_eq!(
        objects.admit(page.clone(), wrong_offset),
        Err(ObjectPageAdmissionError::Malformed(
            ObjectContractError::ScalarBoundaryProofMismatch {
                anchor: ByteOffset::new(1)
            }
        ))
    );
    assert_eq!(format!("{objects:?}"), prior_fingerprint);
    assert_eq!(objects.counts(), prior_counts);

    let other_revision =
        RangeResidency::new(binding(2), ResidencyLimits::new(1, 4096, 1, 16).unwrap());
    let page = page_for(&mut objects, 2, 2, ObjectPurpose::Viewport, 0, 2);
    assert_eq!(
        other_revision.prove_object_page_anchors(objects.binding(), &page),
        Err(ObjectAnchorProofError::Stale(page.key()))
    );
    let stale_proof = text
        .prove_object_page_anchors(objects.binding(), &proof_page)
        .unwrap();
    let prior_fingerprint = format!("{objects:?}");
    let prior_counts = objects.counts();
    assert_eq!(
        objects.admit(page.clone(), stale_proof),
        Err(ObjectPageAdmissionError::Malformed(
            ObjectContractError::ScalarBoundaryProofMismatch {
                anchor: ByteOffset::new(0)
            }
        ))
    );
    assert_eq!(format!("{objects:?}"), prior_fingerprint);
    assert_eq!(objects.counts(), prior_counts);
    objects
        .admit(page.clone(), anchor_proofs(&text, &page))
        .unwrap();
}

#[test]
fn end_edge_proof_cannot_cross_an_equal_id_revision_with_another_extent() {
    let large_text = text_residency("é234567890abcde");
    assert_eq!(
        large_text.prove_scalar_boundary(ByteOffset::new(1)),
        Err(ScalarBoundaryProofError::NotScalarBoundary(
            ByteOffset::new(1)
        ))
    );

    let smaller_binding = RangeBinding::new(
        BindingId::new(7),
        SourceRevision::new(1),
        LogicalExtent::new(1, 1),
    );
    let smaller_text = RangeResidency::new(
        smaller_binding,
        ResidencyLimits::new(1, 4096, 1, 16).unwrap(),
    );
    let endpoint = smaller_text
        .prove_scalar_boundary(ByteOffset::new(1))
        .unwrap();
    assert_eq!(endpoint.range_binding(), smaller_binding);

    let mut objects = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let page = page_for(&mut objects, 1, 1, ObjectPurpose::Viewport, 1, 1);
    assert_eq!(
        smaller_text.prove_object_page_anchors(objects.binding(), &page),
        Err(ObjectAnchorProofError::Stale(page.key()))
    );

    let smaller_batch = smaller_text
        .prove_object_page_anchors(smaller_binding, &page)
        .unwrap();
    assert_eq!(smaller_batch.range_binding(), smaller_binding);
    let prior_fingerprint = format!("{objects:?}");
    let prior_counts = objects.counts();
    assert_eq!(
        objects.admit(page.clone(), smaller_batch),
        Err(ObjectPageAdmissionError::Malformed(
            ObjectContractError::ScalarBoundaryProofMismatch {
                anchor: ByteOffset::new(1)
            }
        ))
    );
    assert_eq!(format!("{objects:?}"), prior_fingerprint);
    assert_eq!(objects.counts(), prior_counts);
}

fn facts_at(anchors: &[u64]) -> Vec<InlineObjectFact> {
    anchors
        .iter()
        .enumerate()
        .map(|(index, anchor)| fact((index + 1) as u128, *anchor, ((index + 1) * 10) as u128))
        .collect()
}

fn exact_key_shape_case(proof_anchors: &[u64], target_anchors: &[u64]) -> ObjectPageAdmissionError {
    let text = text_residency("0123456789abcdef");
    let mut residency = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let demand = ObjectDemandEnvelope::range(
        ByteRange::from_u64(4, 8).unwrap(),
        None,
        ObjectDirection::Forward,
        4,
        8192,
    )
    .unwrap();
    let request = requested_key(
        residency
            .demand(ObjectRequestId::new(1), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectRequestId::new(1),
    );
    let make_page = |anchors: &[u64]| {
        ObjectPage::new(
            ObjectPageId::new(1),
            request,
            facts_at(anchors),
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap()
    };
    let proof_page = make_page(proof_anchors);
    let target = make_page(target_anchors);
    let proofs = text
        .prove_object_page_anchors(residency.binding(), &proof_page)
        .unwrap();
    let prior_fingerprint = format!("{residency:?}");
    let prior_counts = residency.counts();
    let error = residency.admit(target.clone(), proofs).unwrap_err();
    assert_eq!(format!("{residency:?}"), prior_fingerprint);
    assert_eq!(residency.counts(), prior_counts);
    residency
        .admit(target.clone(), anchor_proofs(&text, &target))
        .unwrap();
    error
}

#[test]
fn exact_page_key_batches_reject_wrong_missing_and_extra_anchor_proofs() {
    for (proof_anchors, target_anchors, mismatch) in [
        (&[8][..], &[4][..], 4),
        (&[4][..], &[4, 8][..], 8),
        (&[4, 8][..], &[4][..], 8),
    ] {
        let error = exact_key_shape_case(proof_anchors, target_anchors);
        assert_eq!(
            error,
            ObjectPageAdmissionError::Malformed(ObjectContractError::ScalarBoundaryProofMismatch {
                anchor: ByteOffset::new(mismatch)
            })
        );
    }
}

#[test]
fn repeated_object_anchor_mints_one_nonduplicated_proof_and_admits() {
    let text = text_residency("0123456789abcdef");
    let mut residency = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let demand = anchor_demand(4, None, ObjectDirection::Forward, 2, 8192);
    let request = requested_key(
        residency
            .demand(ObjectRequestId::new(1), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectRequestId::new(1),
    );
    let page = ObjectPage::new(
        ObjectPageId::new(1),
        request,
        vec![fact(1, 4, 10), fact(2, 4, 20)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = text
        .prove_object_page_anchors(residency.binding(), &page)
        .unwrap();
    assert_eq!(proofs.len(), 1);
    assert_eq!(
        residency.admit(page, proofs),
        Ok(ObjectPageAdmission::Admitted {
            page: ObjectPageId::new(1),
            evicted_pages: 0,
            evicted_objects: 0,
        })
    );
    assert_eq!(residency.counts().resident_objects, 2);
}

#[test]
fn equal_page_id_reconciles_exact_payload_and_rejects_changed_facts() {
    let text = text_residency("0123456789abcdef");
    let mut residency = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let first = page_for(&mut residency, 1, 7, ObjectPurpose::Viewport, 5, 1);
    let proofs = anchor_proofs(&text, &first);
    residency.admit(first, proofs).unwrap();
    let before = residency.counts();

    let repeated = page_for(&mut residency, 2, 7, ObjectPurpose::Caret, 5, 1);
    let repeated_key = repeated.key();
    let proofs = anchor_proofs(&text, &repeated);
    assert_eq!(
        residency.admit(repeated, proofs),
        Ok(ObjectPageAdmission::Reconciled {
            page: ObjectPageId::new(7),
            evicted_pages: 0,
            evicted_objects: 0,
        })
    );
    assert_eq!(residency.counts(), before);
    assert_eq!(
        residency.page_by_id(ObjectPageId::new(7)).unwrap().key(),
        repeated_key
    );

    let demand = anchor_demand(5, None, ObjectDirection::Forward, 1, 4096);
    let request = requested_key(
        residency
            .demand(ObjectRequestId::new(3), ObjectPurpose::Selection, demand)
            .unwrap(),
        ObjectRequestId::new(3),
    );
    let changed = ObjectPage::new(
        ObjectPageId::new(7),
        request,
        vec![InlineObjectFact::new(
            InlineObjectId::new(1),
            ByteOffset::new(5),
            InlineObjectOrder::new(10),
            "changed",
            presentation(1, "object"),
        )],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = anchor_proofs(&text, &changed);
    let prior_fingerprint = format!("{residency:?}");
    let prior_counts = residency.counts();
    assert_eq!(
        residency.admit(changed.clone(), proofs),
        Err(ObjectPageAdmissionError::Malformed(
            ObjectContractError::ConflictingPageIdentity {
                page: ObjectPageId::new(7)
            }
        ))
    );
    assert_eq!(format!("{residency:?}"), prior_fingerprint);
    assert_eq!(residency.counts(), prior_counts);

    let retry = ObjectPage::new(
        ObjectPageId::new(7),
        changed.key(),
        vec![fact(1, 5, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    residency
        .admit(retry.clone(), anchor_proofs(&text, &retry))
        .unwrap();
    assert_eq!(residency.counts(), before);
}

#[test]
fn exact_object_demand_coalesces_pending_and_reuses_resident() {
    let text = text_residency("0123456789abcdef");
    let demand = anchor_demand(5, None, ObjectDirection::Forward, 1, 4096);
    let mut admitted = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let first = requested_key(
        admitted
            .demand(ObjectRequestId::new(1), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectRequestId::new(1),
    );
    assert_eq!(
        admitted
            .demand(ObjectRequestId::new(2), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectDemand::Coalesced(first)
    );
    let page = ObjectPage::new(
        ObjectPageId::new(1),
        first,
        vec![fact(1, 5, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let proofs = anchor_proofs(&text, &page);
    admitted.admit(page, proofs).unwrap();
    assert_eq!(
        admitted
            .demand(ObjectRequestId::new(2), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectDemand::Resident(ObjectPageId::new(1))
    );

    let mut failed = ObjectResidency::new(
        binding(1),
        PresentationGeneration::new(4),
        residency_limits(),
    );
    let first = requested_key(
        failed
            .demand(ObjectRequestId::new(10), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectRequestId::new(10),
    );
    assert_eq!(
        failed
            .demand(ObjectRequestId::new(11), ObjectPurpose::Viewport, demand)
            .unwrap(),
        ObjectDemand::Coalesced(first)
    );
    assert_eq!(
        failed.settle(first, ObjectPageFailure::Unavailable),
        ObjectPageSettlement::Settled(ObjectPageFailure::Unavailable)
    );
    assert_eq!(
        requested_key(
            failed
                .demand(ObjectRequestId::new(11), ObjectPurpose::Viewport, demand)
                .unwrap(),
            ObjectRequestId::new(11),
        )
        .demand(),
        demand
    );
}
