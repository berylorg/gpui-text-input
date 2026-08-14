use gpui_text_input::{
    BindingId, ByteOffset, ByteRange, LogicalExtent, PageAdmissionError, PageDemand,
    PageDemandEnvelope, PageDirection, PageEdgeFact, PageFailure, PageId, PagePurpose,
    PageRequestId, PageRequestKey, PageSettlement, RangeBinding, RangeContractError, RangePage,
    RangeResidency, ResidencyLimitKind, ResidencyLimits, SourceRevision,
};

fn binding(revision: u64, bytes: u64) -> RangeBinding {
    RangeBinding::new(
        BindingId::new(7),
        SourceRevision::new(revision),
        LogicalExtent::new(bytes, u64::from(bytes != 0)),
    )
}

fn limits(pages: usize, bytes: usize, pending: usize, pending_bytes: u64) -> ResidencyLimits {
    ResidencyLimits::new(pages, bytes, pending, pending_bytes).unwrap()
}

fn adjacent(anchor: u64, direction: PageDirection, cap: u64) -> PageDemandEnvelope {
    PageDemandEnvelope::Adjacent {
        anchor: ByteOffset::new(anchor),
        direction,
        max_payload_bytes: cap,
    }
}

fn requested_key(demand: PageDemand) -> PageRequestKey {
    match demand {
        PageDemand::Requested(request) => request.key(),
        other => panic!("expected request, got {other:?}"),
    }
}

#[test]
fn coalesced_demand_redemands_as_resident_or_its_own_exact_request_after_settlement() {
    let demand = adjacent(0, PageDirection::Forward, 4);

    let mut admitted = RangeResidency::new(binding(1, 8), limits(2, 16, 2, 8));
    let first = requested_key(
        admitted
            .demand(PageRequestId::new(1), PagePurpose::GeometryTarget, demand)
            .unwrap(),
    );
    assert_eq!(
        admitted
            .demand(PageRequestId::new(2), PagePurpose::GeometryTarget, demand)
            .unwrap(),
        PageDemand::Coalesced(first)
    );
    admitted
        .admit(page(first, 7, (0, 4), "abcd", 8).unwrap())
        .unwrap();
    assert_eq!(
        admitted
            .demand(PageRequestId::new(2), PagePurpose::GeometryTarget, demand)
            .unwrap(),
        PageDemand::ResidentAdjacent(PageId::new(7))
    );

    let mut failed = RangeResidency::new(binding(1, 8), limits(2, 16, 2, 8));
    let first = requested_key(
        failed
            .demand(PageRequestId::new(10), PagePurpose::GeometryTarget, demand)
            .unwrap(),
    );
    assert_eq!(
        failed
            .demand(PageRequestId::new(11), PagePurpose::GeometryTarget, demand)
            .unwrap(),
        PageDemand::Coalesced(first)
    );
    assert_eq!(
        failed.settle(first, PageFailure::Unavailable),
        PageSettlement::Settled(PageFailure::Unavailable)
    );
    let redemanded = requested_key(
        failed
            .demand(PageRequestId::new(11), PagePurpose::GeometryTarget, demand)
            .unwrap(),
    );
    assert_eq!(redemanded.id(), PageRequestId::new(11));
    assert_eq!(redemanded.demand(), demand);
}

fn page(
    key: PageRequestKey,
    id: u64,
    range: (u64, u64),
    text: &str,
    extent: u64,
) -> Result<RangePage, RangeContractError> {
    RangePage::new(
        PageId::new(id),
        key,
        ByteRange::from_u64(range.0, range.1).unwrap(),
        text.to_owned(),
        vec![],
        if range.0 == 0 {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if range.1 == extent {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        range.1 == extent,
    )
}

#[test]
fn adjacent_envelopes_enforce_minimum_cap_anchor_progress_and_source_selected_edge() {
    assert!(matches!(
        PageRequestKey::adjacent(
            PageRequestId::new(1),
            BindingId::new(7),
            SourceRevision::new(1),
            PagePurpose::Viewport,
            ByteOffset::new(0),
            PageDirection::Forward,
            3,
        ),
        Err(RangeContractError::PagePayloadLimitTooSmall {
            max_payload_bytes: 3
        })
    ));
    let exact = PageRequestKey::adjacent(
        PageRequestId::new(2),
        BindingId::new(7),
        SourceRevision::new(1),
        PagePurpose::Viewport,
        ByteOffset::new(0),
        PageDirection::Forward,
        4,
    )
    .unwrap();
    assert!(page(exact, 1, (0, 4), "éab", 10).is_ok());
    assert!(page(exact, 2, (0, 3), "éa", 10).is_ok());
    assert!(matches!(
        page(exact, 3, (1, 4), "abc", 10),
        Err(RangeContractError::ReturnedRangeOutsideEnvelope { .. })
    ));
    assert!(matches!(
        page(exact, 4, (0, 0), "", 10),
        Err(RangeContractError::NonProgressingPage { .. })
    ));

    let backward = PageRequestKey::adjacent(
        PageRequestId::new(3),
        BindingId::new(7),
        SourceRevision::new(1),
        PagePurpose::Caret,
        ByteOffset::new(6),
        PageDirection::Backward,
        4,
    )
    .unwrap();
    assert!(page(backward, 5, (2, 6), "abcd", 10).is_ok());
    assert!(matches!(
        page(backward, 6, (2, 5), "abc", 10),
        Err(RangeContractError::ReturnedRangeOutsideEnvelope { .. })
    ));
}

#[test]
fn validation_proves_exact_boundaries_and_rejects_inside_scalar_without_rounding() {
    let key = PageRequestKey::validation(
        PageRequestId::new(10),
        BindingId::new(7),
        SourceRevision::new(1),
        PagePurpose::Restoration,
        ByteOffset::new(1),
        4,
    )
    .unwrap();
    let inside = page(key, 10, (0, 4), "éab", 4).unwrap();
    assert_eq!(inside.candidate_is_boundary(), Some(false));

    let key = PageRequestKey::validation(
        PageRequestId::new(11),
        BindingId::new(7),
        SourceRevision::new(1),
        PagePurpose::Restoration,
        ByteOffset::new(2),
        4,
    )
    .unwrap();
    let boundary = page(key, 11, (0, 4), "éab", 4).unwrap();
    assert_eq!(boundary.candidate_is_boundary(), Some(true));

    let end = PageRequestKey::validation(
        PageRequestId::new(12),
        BindingId::new(7),
        SourceRevision::new(1),
        PagePurpose::Restoration,
        ByteOffset::new(4),
        4,
    )
    .unwrap();
    assert_eq!(
        page(end, 12, (4, 4), "", 4)
            .unwrap()
            .candidate_is_boundary(),
        Some(true)
    );
    let empty = PageRequestKey::validation(
        PageRequestId::new(13),
        BindingId::new(7),
        SourceRevision::new(1),
        PagePurpose::Restoration,
        ByteOffset::new(0),
        4,
    )
    .unwrap();
    assert_eq!(
        page(empty, 13, (0, 0), "", 0)
            .unwrap()
            .candidate_is_boundary(),
        Some(true)
    );

    let uncovered = PageRequestKey::validation(
        PageRequestId::new(14),
        BindingId::new(7),
        SourceRevision::new(1),
        PagePurpose::Restoration,
        ByteOffset::new(5),
        4,
    )
    .unwrap();
    assert!(matches!(
        page(uncovered, 14, (0, 4), "abcd", 8),
        Err(RangeContractError::ReturnedRangeOutsideEnvelope { .. })
    ));
}

#[test]
fn residency_rejects_mismatched_stale_duplicate_and_over_cap_responses_and_releases_pending() {
    let mut residency = RangeResidency::new(binding(1, 10), limits(2, 16, 2, 8));
    let key = requested_key(
        residency
            .demand(
                PageRequestId::new(1),
                PagePurpose::Viewport,
                adjacent(0, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    assert_eq!(residency.counts().pending_bytes, 4);

    let wrong = PageRequestKey::adjacent(
        PageRequestId::new(1),
        key.binding(),
        key.revision(),
        PagePurpose::Caret,
        ByteOffset::new(0),
        PageDirection::Forward,
        4,
    )
    .unwrap();
    assert!(matches!(
        residency.admit(page(wrong, 1, (0, 4), "abcd", 10).unwrap()),
        Err(PageAdmissionError::Unavailable(_))
    ));
    assert_eq!(residency.counts().pending_bytes, 4);

    residency
        .admit(page(key, 2, (0, 3), "abc", 10).unwrap())
        .unwrap();
    assert_eq!(residency.counts().pending_bytes, 0);
    assert!(matches!(
        residency.admit(page(key, 3, (0, 2), "ab", 10).unwrap()),
        Err(PageAdmissionError::Unavailable(_))
    ));

    let stale = PageRequestKey::adjacent(
        PageRequestId::new(9),
        key.binding(),
        SourceRevision::new(99),
        PagePurpose::Viewport,
        ByteOffset::new(3),
        PageDirection::Forward,
        4,
    )
    .unwrap();
    assert!(matches!(
        residency.admit(page(stale, 4, (3, 6), "def", 10).unwrap()),
        Err(PageAdmissionError::Stale(_))
    ));

    let over = PageRequestKey::adjacent(
        PageRequestId::new(5),
        key.binding(),
        key.revision(),
        PagePurpose::Viewport,
        ByteOffset::new(3),
        PageDirection::Forward,
        4,
    )
    .unwrap();
    assert!(matches!(
        page(over, 5, (3, 8), "defgh", 10),
        Err(RangeContractError::ReturnedRangeOutsideEnvelope { .. })
    ));
}

#[test]
fn cancellation_settlement_rebind_and_dispose_release_exact_capacity() {
    let mut residency = RangeResidency::new(binding(1, 10), limits(2, 16, 2, 8));
    let first = requested_key(
        residency
            .demand(
                PageRequestId::new(1),
                PagePurpose::Viewport,
                adjacent(0, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    let second = requested_key(
        residency
            .demand(
                PageRequestId::new(2),
                PagePurpose::Caret,
                adjacent(10, PageDirection::Backward, 4),
            )
            .unwrap(),
    );
    assert_eq!(residency.counts().pending_bytes, 8);
    assert_eq!(
        residency.cancel(first),
        PageSettlement::Settled(PageFailure::Cancelled)
    );
    assert_eq!(residency.cancel(first), PageSettlement::AlreadyCancelled);
    assert_eq!(residency.counts().pending_bytes, 4);
    assert_eq!(residency.rebind(binding(2, 10)), vec![second]);
    assert_eq!(residency.counts(), Default::default());

    let third = requested_key(
        residency
            .demand(
                PageRequestId::new(3),
                PagePurpose::Viewport,
                adjacent(0, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    assert_eq!(residency.dispose(), vec![third]);
    assert_eq!(residency.counts(), Default::default());
}

#[test]
fn pending_cap_is_charged_by_envelope_and_exact_cap_is_accepted() {
    let mut residency = RangeResidency::new(binding(1, 20), limits(2, 16, 2, 8));
    assert!(matches!(
        residency.demand(
            PageRequestId::new(1),
            PagePurpose::Viewport,
            adjacent(0, PageDirection::Forward, 8)
        ),
        Ok(PageDemand::Requested(_))
    ));
    assert!(matches!(
        residency.demand(
            PageRequestId::new(2),
            PagePurpose::Caret,
            adjacent(20, PageDirection::Backward, 4)
        ),
        Err(gpui_text_input::PageDemandError::LimitExceeded(
            ResidencyLimitKind::PendingBytes
        ))
    ));
}

#[test]
fn resident_reuse_obeys_later_cap_and_returns_explicit_cached_validation_proof() {
    let mut residency = RangeResidency::new(binding(1, 8), limits(3, 24, 3, 24));
    let wide = requested_key(
        residency
            .demand(
                PageRequestId::new(1),
                PagePurpose::Viewport,
                adjacent(0, PageDirection::Forward, 8),
            )
            .unwrap(),
    );
    residency
        .admit(page(wide, 1, (0, 8), "éabcdef", 8).unwrap())
        .unwrap();

    assert_eq!(
        residency
            .demand(
                PageRequestId::new(2),
                PagePurpose::Caret,
                adjacent(0, PageDirection::Forward, 8),
            )
            .unwrap(),
        PageDemand::ResidentAdjacent(PageId::new(1)),
    );
    let narrower = residency
        .demand(
            PageRequestId::new(3),
            PagePurpose::Caret,
            adjacent(0, PageDirection::Forward, 7),
        )
        .unwrap();
    assert!(matches!(narrower, PageDemand::Requested(_)));
    assert_eq!(residency.counts().pending_bytes, 7);
    let PageDemand::Requested(narrower) = narrower else {
        unreachable!()
    };
    assert_eq!(
        residency.cancel(narrower.key()),
        PageSettlement::Settled(PageFailure::Cancelled)
    );
    assert_eq!(residency.counts().pending_bytes, 0);

    assert_eq!(
        residency
            .demand(
                PageRequestId::new(4),
                PagePurpose::Restoration,
                PageDemandEnvelope::Validation {
                    candidate: ByteOffset::new(2),
                    max_payload_bytes: 8,
                },
            )
            .unwrap(),
        PageDemand::ResidentValidation {
            page: PageId::new(1),
            candidate_is_boundary: true,
        },
    );
    assert_eq!(
        residency
            .demand(
                PageRequestId::new(5),
                PagePurpose::Restoration,
                PageDemandEnvelope::Validation {
                    candidate: ByteOffset::new(1),
                    max_payload_bytes: 8,
                },
            )
            .unwrap(),
        PageDemand::ResidentValidation {
            page: PageId::new(1),
            candidate_is_boundary: false,
        },
    );
    assert_eq!(residency.counts().resident_pages, 1);
    assert_eq!(residency.counts().pending_requests, 0);
}

#[test]
fn empty_cached_pages_only_satisfy_adjacent_demand_at_the_matching_document_edge() {
    let mut end_residency = RangeResidency::new(binding(1, 4), limits(3, 16, 3, 24));
    let validation = requested_key(
        end_residency
            .demand(
                PageRequestId::new(1),
                PagePurpose::Restoration,
                PageDemandEnvelope::Validation {
                    candidate: ByteOffset::new(4),
                    max_payload_bytes: 4,
                },
            )
            .unwrap(),
    );
    end_residency
        .admit(page(validation, 1, (4, 4), "", 4).unwrap())
        .unwrap();
    assert_eq!(
        end_residency
            .demand(
                PageRequestId::new(2),
                PagePurpose::Caret,
                adjacent(4, PageDirection::Forward, 4),
            )
            .unwrap(),
        PageDemand::ResidentAdjacent(PageId::new(1))
    );
    assert!(matches!(
        end_residency
            .demand(
                PageRequestId::new(3),
                PagePurpose::Caret,
                adjacent(4, PageDirection::Backward, 4),
            )
            .unwrap(),
        PageDemand::Requested(_)
    ));

    let mut start_residency = RangeResidency::new(binding(1, 4), limits(3, 16, 3, 24));
    let validation = requested_key(
        start_residency
            .demand(
                PageRequestId::new(1),
                PagePurpose::Restoration,
                PageDemandEnvelope::Validation {
                    candidate: ByteOffset::new(0),
                    max_payload_bytes: 4,
                },
            )
            .unwrap(),
    );
    start_residency
        .admit(page(validation, 2, (0, 0), "", 4).unwrap())
        .unwrap();
    assert_eq!(
        start_residency
            .demand(
                PageRequestId::new(2),
                PagePurpose::Caret,
                adjacent(0, PageDirection::Backward, 4),
            )
            .unwrap(),
        PageDemand::ResidentAdjacent(PageId::new(2))
    );
    assert!(matches!(
        start_residency
            .demand(
                PageRequestId::new(3),
                PagePurpose::Caret,
                adjacent(0, PageDirection::Forward, 4),
            )
            .unwrap(),
        PageDemand::Requested(_)
    ));
}

#[test]
fn request_ids_cannot_be_reused_after_admission_settlement_cancellation_or_dispose() {
    let mut residency = RangeResidency::new(binding(1, 8), limits(2, 16, 2, 8));
    let admitted = requested_key(
        residency
            .demand(
                PageRequestId::new(1),
                PagePurpose::Viewport,
                adjacent(0, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    residency
        .admit(page(admitted, 1, (0, 4), "abcd", 8).unwrap())
        .unwrap();
    assert!(matches!(
        residency.demand(
            PageRequestId::new(1),
            PagePurpose::Caret,
            adjacent(4, PageDirection::Forward, 4)
        ),
        Err(gpui_text_input::PageDemandError::RequestIdInUse(_))
    ));
    let settled = requested_key(
        residency
            .demand(
                PageRequestId::new(2),
                PagePurpose::Caret,
                adjacent(4, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    residency.settle(settled, PageFailure::Unavailable);
    assert!(matches!(
        residency.demand(
            PageRequestId::new(2),
            PagePurpose::Caret,
            adjacent(4, PageDirection::Forward, 4)
        ),
        Err(gpui_text_input::PageDemandError::RequestIdInUse(_))
    ));
    let cancelled = requested_key(
        residency
            .demand(
                PageRequestId::new(3),
                PagePurpose::Caret,
                adjacent(4, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    residency.cancel(cancelled);
    assert!(matches!(
        residency.demand(
            PageRequestId::new(3),
            PagePurpose::Caret,
            adjacent(4, PageDirection::Forward, 4)
        ),
        Err(gpui_text_input::PageDemandError::RequestIdInUse(_))
    ));
    let disposed = requested_key(
        residency
            .demand(
                PageRequestId::new(4),
                PagePurpose::Caret,
                adjacent(4, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    assert_eq!(residency.dispose(), vec![disposed]);
    assert!(matches!(
        residency.demand(
            PageRequestId::new(4),
            PagePurpose::Caret,
            adjacent(4, PageDirection::Forward, 4)
        ),
        Err(gpui_text_input::PageDemandError::RequestIdInUse(_))
    ));

    residency.rebind(binding(2, 8));
    assert!(matches!(
        residency.demand(
            PageRequestId::new(1),
            PagePurpose::Viewport,
            adjacent(0, PageDirection::Forward, 4)
        ),
        Ok(PageDemand::Requested(_))
    ));
    assert!(matches!(
        residency.admit(page(admitted, 9, (0, 4), "abcd", 8).unwrap()),
        Err(PageAdmissionError::Stale(_))
    ));
}

#[test]
fn identical_rebind_releases_capacity_without_reopening_request_identity() {
    let current = binding(1, 8);
    let mut residency = RangeResidency::new(current, limits(2, 16, 2, 8));
    let admitted = requested_key(
        residency
            .demand(
                PageRequestId::new(7),
                PagePurpose::Viewport,
                adjacent(0, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    let late_admitted_page = page(admitted, 1, (0, 4), "abcd", 8).unwrap();
    residency.admit(late_admitted_page.clone()).unwrap();
    assert_eq!(residency.counts().resident_pages, 1);
    assert_eq!(residency.counts().resident_bytes, 4);

    assert!(residency.rebind(current).is_empty());
    assert_eq!(residency.counts(), Default::default());
    assert!(matches!(
        residency.demand(
            PageRequestId::new(7),
            PagePurpose::Caret,
            adjacent(4, PageDirection::Forward, 4)
        ),
        Err(gpui_text_input::PageDemandError::RequestIdInUse(_))
    ));
    assert!(matches!(
        residency.admit(late_admitted_page),
        Err(PageAdmissionError::Unavailable(_))
    ));

    let pending = requested_key(
        residency
            .demand(
                PageRequestId::new(8),
                PagePurpose::Caret,
                adjacent(4, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    assert_eq!(residency.counts().pending_requests, 1);
    assert_eq!(residency.counts().pending_bytes, 4);
    assert_eq!(residency.rebind(current), vec![pending]);
    assert_eq!(residency.counts(), Default::default());
    assert!(matches!(
        residency.demand(
            PageRequestId::new(8),
            PagePurpose::Viewport,
            adjacent(0, PageDirection::Forward, 4)
        ),
        Err(gpui_text_input::PageDemandError::RequestIdInUse(_))
    ));
    assert!(matches!(
        residency.admit(page(pending, 2, (4, 8), "efgh", 8).unwrap()),
        Err(PageAdmissionError::Unavailable(_))
    ));
}

#[test]
fn extent_only_rebind_releases_capacity_without_reopening_request_identity() {
    let mut residency = RangeResidency::new(binding(1, 8), limits(2, 16, 2, 8));
    let pending = requested_key(
        residency
            .demand(
                PageRequestId::new(9),
                PagePurpose::Viewport,
                adjacent(0, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    let late = page(pending, 1, (0, 4), "abcd", 8).unwrap();
    assert_eq!(residency.counts().pending_requests, 1);
    assert_eq!(residency.counts().pending_bytes, 4);

    assert_eq!(residency.rebind(binding(1, 12)), vec![pending]);
    assert_eq!(residency.counts(), Default::default());
    assert!(matches!(
        residency.demand(
            PageRequestId::new(9),
            PagePurpose::Caret,
            adjacent(4, PageDirection::Forward, 4)
        ),
        Err(gpui_text_input::PageDemandError::RequestIdInUse(_))
    ));
    assert!(matches!(
        residency.admit(late),
        Err(PageAdmissionError::Unavailable(_))
    ));

    let admitted = requested_key(
        residency
            .demand(
                PageRequestId::new(10),
                PagePurpose::Viewport,
                adjacent(0, PageDirection::Forward, 4),
            )
            .unwrap(),
    );
    residency
        .admit(page(admitted, 2, (0, 4), "abcd", 12).unwrap())
        .unwrap();
    assert_eq!(residency.counts().resident_pages, 1);
    assert_eq!(residency.counts().resident_bytes, 4);
    assert!(residency.rebind(binding(1, 16)).is_empty());
    assert_eq!(residency.counts(), Default::default());
    assert!(matches!(
        residency.demand(
            PageRequestId::new(10),
            PagePurpose::Viewport,
            adjacent(0, PageDirection::Forward, 4)
        ),
        Err(gpui_text_input::PageDemandError::RequestIdInUse(_))
    ));
    assert!(matches!(
        residency.admit(page(admitted, 3, (0, 4), "abcd", 16).unwrap()),
        Err(PageAdmissionError::Unavailable(_))
    ));
}
