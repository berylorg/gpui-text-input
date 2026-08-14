use gpui_text_input::{
    AtomFact, AtomId, BindingId, ByteRange, LogicalExtent, PageAdmissionError, PageDemand,
    PageDemandEnvelope, PageDirection, PageEdgeFact, PageFailure, PageId, PagePurpose,
    PageRequestId, PageRequestKey, PageSettlement, RangeBinding, RangeContractError, RangePage,
    RangeResidency, ResidencyLimits, SourceRevision,
};

fn binding() -> RangeBinding {
    RangeBinding::new(
        BindingId::new(1),
        SourceRevision::new(1),
        LogicalExtent::new(10, 1),
    )
}

fn residency() -> RangeResidency {
    RangeResidency::new(binding(), ResidencyLimits::new(4, 128, 4, 40).unwrap())
}

fn demand(residency: &mut RangeResidency, id: u64, start: u64, end: u64) -> PageRequestKey {
    match residency
        .demand(
            PageRequestId::new(id),
            PagePurpose::Viewport,
            PageDemandEnvelope::Adjacent {
                anchor: gpui_text_input::ByteOffset::new(start),
                direction: PageDirection::Forward,
                max_payload_bytes: (end - start).max(4),
            },
        )
        .unwrap()
    {
        PageDemand::Requested(request) => request.key(),
        other => panic!("expected exact request, got {other:?}"),
    }
}

fn page(
    id: u64,
    key: PageRequestKey,
    text: &str,
    atoms: Vec<AtomFact>,
) -> Result<RangePage, RangeContractError> {
    RangePage::new(
        PageId::new(id),
        key,
        ByteRange::from_u64(
            match key.demand() {
                PageDemandEnvelope::Adjacent { anchor, .. } => anchor.get(),
                _ => unreachable!(),
            },
            match key.demand() {
                PageDemandEnvelope::Adjacent { anchor, .. } => anchor.get() + text.len() as u64,
                _ => unreachable!(),
            },
        )
        .unwrap(),
        text.to_owned(),
        atoms,
        if matches!(key.demand(), PageDemandEnvelope::Adjacent { anchor, .. } if anchor.get() == 0)
        {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        if matches!(key.demand(), PageDemandEnvelope::Adjacent { anchor, .. } if anchor.get() + text.len() as u64 == 10)
        {
            PageEdgeFact::DocumentBoundary
        } else {
            PageEdgeFact::Continues
        },
        matches!(key.demand(), PageDemandEnvelope::Adjacent { anchor, .. } if anchor.get() + text.len() as u64 == 10),
    )
}

fn atom(id: u64, global: (u64, u64), fragment: (u64, u64), fallback: &str) -> AtomFact {
    AtomFact::new(
        AtomId::new(id),
        ByteRange::from_u64(global.0, global.1).unwrap(),
        ByteRange::from_u64(fragment.0, fragment.1).unwrap(),
        fallback,
    )
}

#[test]
fn one_atom_reconciles_across_two_exact_pages_without_a_registry() {
    let mut residency = residency();
    let first_key = demand(&mut residency, 1, 0, 5);
    let second_key = demand(&mut residency, 2, 5, 10);
    let first = page(
        1,
        first_key,
        "abcde",
        vec![atom(7, (2, 8), (2, 5), "asset")],
    )
    .unwrap();
    let second = page(
        2,
        second_key,
        "fghij",
        vec![atom(7, (2, 8), (5, 8), "asset")],
    )
    .unwrap();

    assert!(first.atoms()[0].reconciles_with(&second.atoms()[0]));
    residency.admit(first).unwrap();
    residency.admit(second).unwrap();

    assert_eq!(residency.counts().resident_pages, 2);
    assert_eq!(residency.counts().resident_bytes, 20);
    let fragments: Vec<_> = residency
        .resident_pages()
        .map(|page| page.atoms()[0].fragment_range())
        .collect();
    assert_eq!(
        fragments,
        vec![
            ByteRange::from_u64(2, 5).unwrap(),
            ByteRange::from_u64(5, 8).unwrap()
        ]
    );
}

#[test]
fn residency_rejects_global_ranges_outside_the_revision_and_releases_pending() {
    let mut residency = residency();
    let key = demand(&mut residency, 1, 5, 10);
    let outside = page(1, key, "fghij", vec![atom(7, (8, 12), (8, 10), "asset")]).unwrap();
    assert!(matches!(
        residency.admit(outside),
        Err(PageAdmissionError::Malformed(
            RangeContractError::MalformedAtomRange { .. }
        ))
    ));
    assert_eq!(residency.counts().pending_requests, 0);
    assert_eq!(residency.counts().pending_bytes, 0);
}

#[test]
fn residency_rejects_conflicting_stable_facts_across_adjacent_pages() {
    let mut residency = residency();
    let first_key = demand(&mut residency, 1, 0, 5);
    let second_key = demand(&mut residency, 2, 5, 10);
    residency
        .admit(
            page(
                1,
                first_key,
                "abcde",
                vec![atom(7, (2, 8), (2, 5), "asset")],
            )
            .unwrap(),
        )
        .unwrap();
    let conflict = page(
        2,
        second_key,
        "fghij",
        vec![atom(7, (2, 9), (5, 9), "asset")],
    )
    .unwrap();
    assert_eq!(
        residency.admit(conflict),
        Err(PageAdmissionError::Malformed(
            RangeContractError::ConflictingAtomFacts {
                atom: AtomId::new(7)
            }
        ))
    );
    assert_eq!(residency.counts().pending_requests, 0);
    assert_eq!(residency.counts().resident_pages, 1);
}

#[test]
fn residency_rejects_different_ids_with_overlapping_global_ranges() {
    let mut residency = residency();
    let first_key = demand(&mut residency, 1, 0, 5);
    let second_key = demand(&mut residency, 2, 5, 10);
    residency
        .admit(
            page(
                1,
                first_key,
                "abcde",
                vec![atom(7, (2, 8), (2, 5), "asset")],
            )
            .unwrap(),
        )
        .unwrap();
    let overlap = page(
        2,
        second_key,
        "fghij",
        vec![atom(8, (4, 9), (5, 9), "other")],
    )
    .unwrap();
    assert_eq!(
        residency.admit(overlap),
        Err(PageAdmissionError::Malformed(
            RangeContractError::OverlappingAtomFacts {
                first: AtomId::new(7),
                second: AtomId::new(8),
            }
        ))
    );
    assert_eq!(residency.counts().pending_requests, 0);
    assert_eq!(residency.counts().resident_pages, 1);
}

#[test]
fn page_rejects_inexact_empty_overlapping_and_unordered_fragments() {
    let mut residency = residency();
    let key = demand(&mut residency, 1, 0, 5);
    let inexact = page(1, key, "abcde", vec![atom(1, (2, 8), (2, 4), "x")]);
    assert!(matches!(
        inexact,
        Err(RangeContractError::MalformedAtomRange { .. })
    ));

    let empty = page(2, key, "abcde", vec![atom(1, (2, 2), (2, 2), "x")]);
    assert!(matches!(
        empty,
        Err(RangeContractError::MalformedAtomRange { .. })
    ));

    let overlapping = page(
        3,
        key,
        "abcde",
        vec![atom(1, (1, 4), (1, 4), "a"), atom(2, (3, 5), (3, 5), "b")],
    );
    assert!(matches!(
        overlapping,
        Err(RangeContractError::MalformedAtomRange { .. })
    ));

    let unordered = page(
        4,
        key,
        "abcde",
        vec![atom(2, (3, 5), (3, 5), "b"), atom(1, (1, 3), (1, 3), "a")],
    );
    assert!(matches!(
        unordered,
        Err(RangeContractError::MalformedAtomRange { .. })
    ));
}

#[test]
fn page_rejects_consistent_duplicate_atom_facts_and_malformed_settlement_releases_capacity() {
    let mut residency = residency();
    let key = demand(&mut residency, 1, 0, 5);
    let duplicate = atom(7, (1, 4), (1, 4), "asset");

    assert_eq!(
        page(1, key, "abcde", vec![duplicate.clone(), duplicate],),
        Err(RangeContractError::DuplicateAtomFact {
            atom: AtomId::new(7),
        })
    );
    assert_eq!(residency.counts().resident_pages, 0);
    assert_eq!(residency.counts().pending_requests, 1);
    assert_eq!(residency.counts().pending_bytes, 5);
    assert_eq!(
        residency.settle(key, PageFailure::Malformed),
        PageSettlement::Settled(PageFailure::Malformed)
    );
    assert_eq!(residency.counts().pending_requests, 0);
    assert_eq!(residency.counts().pending_bytes, 0);
}

#[test]
fn page_rejects_inconsistent_duplicate_atom_facts_and_malformed_settlement_releases_capacity() {
    let mut residency = residency();
    let key = demand(&mut residency, 1, 0, 5);

    assert_eq!(
        page(
            1,
            key,
            "abcde",
            vec![
                atom(7, (0, 2), (0, 2), "first"),
                atom(7, (3, 5), (3, 5), "second"),
            ],
        ),
        Err(RangeContractError::DuplicateAtomFact {
            atom: AtomId::new(7),
        })
    );
    assert_eq!(residency.counts().resident_pages, 0);
    assert_eq!(residency.counts().resident_bytes, 0);
    assert_eq!(
        residency.settle(key, PageFailure::Malformed),
        PageSettlement::Settled(PageFailure::Malformed)
    );
    assert_eq!(residency.counts().pending_requests, 0);
    assert_eq!(residency.counts().pending_bytes, 0);
}
