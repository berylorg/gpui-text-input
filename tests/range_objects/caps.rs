use super::*;

fn page_for(
    residency: &mut ObjectResidency,
    request_id: u64,
    page_id: u64,
    anchor: u64,
    object_id: u128,
) -> ObjectPage {
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

fn admit_pair_at_limits(
    limits: ObjectResidencyLimits,
) -> (ObjectPageAdmission, ObjectResidencyCounts) {
    let text = text_residency("0123456789abcdef");
    let mut residency = ObjectResidency::new(binding(1), PresentationGeneration::new(4), limits);
    for (request, anchor, object) in [(1, 4, 1), (2, 8, 2)] {
        let page = page_for(&mut residency, request, request, anchor, object);
        let proofs = anchor_proofs(&text, &page);
        let admission = residency.admit(page, proofs).unwrap();
        if request == 2 {
            return (admission, residency.counts());
        }
    }
    unreachable!()
}

fn one_object_charge() -> ObjectPageCharge {
    let demand = anchor_demand(4, None, ObjectDirection::Forward, 1, 4096);
    ObjectPage::new(
        ObjectPageId::new(90),
        key(90, demand),
        vec![fact(1, 4, 10)],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap()
    .retained_charge()
}

fn empty_retry_page(page: &ObjectPage) -> ObjectPage {
    ObjectPage::new(
        ObjectPageId::new(2),
        page.key(),
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap()
}

#[test]
fn resident_object_count_cap_evicts_exactly_at_its_independent_boundary() {
    let limits = ObjectResidencyLimits::new(2, 1, 8192, 4096, 2, 2, 8192).unwrap();
    let (admission, counts) = admit_pair_at_limits(limits);
    assert_eq!(
        admission,
        ObjectPageAdmission::Admitted {
            page: ObjectPageId::new(2),
            evicted_pages: 1,
            evicted_objects: 1,
        }
    );
    assert_eq!(counts.resident_pages, 1);
    assert_eq!(counts.resident_objects, 1);
}

#[test]
fn resident_retained_byte_cap_accepts_exact_and_rejects_one_under_independently() {
    let charge = one_object_charge();
    let exact = ObjectResidencyLimits::new(2, 2, charge.bytes(), 4096, 2, 2, 8192).unwrap();
    let (admission, counts) = admit_pair_at_limits(exact);
    assert!(matches!(
        admission,
        ObjectPageAdmission::Admitted {
            evicted_pages: 1,
            ..
        }
    ));
    assert_eq!(counts.resident_bytes, charge.bytes());

    let text = text_residency("0123456789abcdef");
    let one_under = ObjectResidencyLimits::new(2, 2, charge.bytes() - 1, 4096, 2, 2, 8192).unwrap();
    let mut residency = ObjectResidency::new(binding(1), PresentationGeneration::new(4), one_under);
    let page = page_for(&mut residency, 1, 1, 4, 1);
    let proofs = anchor_proofs(&text, &page);
    let retry = empty_retry_page(&page);
    let prior_fingerprint = format!("{residency:?}");
    let prior_counts = residency.counts();
    assert_eq!(
        residency.admit(page, proofs),
        Err(ObjectPageAdmissionError::LimitExceeded(
            ObjectResidencyLimitKind::ResidentBytes
        ))
    );
    assert_eq!(format!("{residency:?}"), prior_fingerprint);
    assert_eq!(residency.counts(), prior_counts);

    let retry_proofs = anchor_proofs(&text, &retry);
    residency.admit(retry, retry_proofs).unwrap();
    assert_eq!(residency.counts().pending_requests, 0);
}

#[test]
fn resident_presentation_byte_cap_accepts_exact_and_rejects_one_under_independently() {
    let charge = one_object_charge();
    let exact =
        ObjectResidencyLimits::new(2, 2, 8192, charge.presentation_bytes(), 2, 2, 8192).unwrap();
    let (admission, counts) = admit_pair_at_limits(exact);
    assert!(matches!(
        admission,
        ObjectPageAdmission::Admitted {
            evicted_pages: 1,
            ..
        }
    ));
    assert_eq!(
        counts.resident_presentation_bytes,
        charge.presentation_bytes()
    );

    let text = text_residency("0123456789abcdef");
    let one_under =
        ObjectResidencyLimits::new(2, 2, 8192, charge.presentation_bytes() - 1, 2, 2, 8192)
            .unwrap();
    let mut residency = ObjectResidency::new(binding(1), PresentationGeneration::new(4), one_under);
    let page = page_for(&mut residency, 1, 1, 4, 1);
    let proofs = anchor_proofs(&text, &page);
    let prior_fingerprint = format!("{residency:?}");
    let prior_counts = residency.counts();
    assert_eq!(
        residency.admit(page.clone(), proofs),
        Err(ObjectPageAdmissionError::LimitExceeded(
            ObjectResidencyLimitKind::ResidentPresentationBytes
        ))
    );
    assert_eq!(format!("{residency:?}"), prior_fingerprint);
    assert_eq!(residency.counts(), prior_counts);

    let retry = empty_retry_page(&page);
    let retry_proofs = anchor_proofs(&text, &retry);
    residency.admit(retry, retry_proofs).unwrap();
    assert_eq!(residency.counts().pending_requests, 0);
}
