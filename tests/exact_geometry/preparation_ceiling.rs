use super::*;
use gpui_text_input::preparation_test_support::{
    PreparationCapacityProbe, enclosing_failure_peak, is_configured_capacity_refusal,
    is_enclosing_capacity_refusal, preparation_remaining_capacity,
    preparation_remaining_capacity_with_baselines, prepare_response,
};

#[test]
fn pre_shaping_allowance_uses_mapped_headroom_without_future_credit() {
    for (current, enclosing, expected) in [
        ((80, 8), (150, 15), (20, 2)),
        ((180, 18), (250, 25), (20, 2)),
        ((0, 0), (70, 7), (20, 2)),
        ((80, 8), (1000, 100), (50, 5)),
        ((80, 8), (180, 14), (50, 1)),
        ((80, 8), (131, 18), (1, 5)),
    ] {
        let occupied = (150, 15);
        let configured = (200, 20);
        let baseline = (100, 10);
        let remaining = preparation_remaining_capacity_with_baselines(
            occupied, configured, enclosing, baseline, current,
        )
        .unwrap();
        assert_eq!(remaining, expected);
        let mut probe =
            PreparationCapacityProbe::with_baselines(configured, enclosing, baseline, current);
        let exact = (occupied.0 + remaining.0, occupied.1 + remaining.1);
        probe.observe_preparation(exact, (0, 0)).unwrap();
        assert!(
            probe
                .observe_preparation((exact.0 + 1, exact.1), (0, 0))
                .is_err()
        );
        assert!(
            probe
                .observe_preparation((exact.0, exact.1 + 1), (0, 0))
                .is_err()
        );
    }
    for (configured, enclosing, configured_refusal) in [
        ((200, 20), (130, 18), false),
        ((200, 20), (180, 13), false),
        ((150, 20), (130, 18), true),
        ((200, 15), (180, 13), true),
    ] {
        let failure = preparation_remaining_capacity_with_baselines(
            (150, 15),
            configured,
            enclosing,
            (100, 10),
            (80, 8),
        )
        .unwrap_err();
        assert_eq!(is_configured_capacity_refusal(&failure), configured_refusal);
        assert_eq!(is_enclosing_capacity_refusal(&failure), !configured_refusal);
        assert_eq!(enclosing_failure_peak(&failure), Some((131, 14)));
        assert_eq!(failure.release(), &ExactGeometryRelease::default());
    }
}

#[test]
fn pre_shaping_mapped_allowance_checks_arithmetic_boundaries() {
    let max = (usize::MAX, usize::MAX);
    for (occupied, baseline, current) in [
        ((usize::MAX, 10), (100, 10), (80, 8)),
        ((100, usize::MAX), (100, 10), (80, 8)),
        ((100, 10), (100, 10), (usize::MAX, 8)),
        ((100, 10), (100, 10), (80, usize::MAX)),
        ((98, 10), (100, 10), (80, 8)),
        ((100, 8), (100, 10), (80, 8)),
        ((99, 10), (100, 10), (80, 8)),
        ((100, 9), (100, 10), (80, 8)),
    ] {
        let failure =
            preparation_remaining_capacity_with_baselines(occupied, max, max, baseline, current)
                .unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
        assert!(!is_configured_capacity_refusal(&failure));
        assert!(!is_enclosing_capacity_refusal(&failure));
    }
    assert_eq!(
        preparation_remaining_capacity_with_baselines(
            (usize::MAX - 1, usize::MAX - 1),
            max,
            max,
            (usize::MAX - 1, usize::MAX - 1),
            (0, 0),
        )
        .unwrap(),
        (1, 1)
    );
    assert_eq!(
        preparation_remaining_capacity_with_baselines(
            (100, 10),
            max,
            max,
            (100, 10),
            (usize::MAX - 1, usize::MAX - 1),
        )
        .unwrap(),
        (1, 1)
    );
}

#[test]
fn prepared_failure_retains_independent_enclosing_peak_evidence() {
    for arithmetic_failure in [false, true] {
        let mut probe =
            PreparationCapacityProbe::with_baselines((200, 20), (130, 13), (100, 10), (80, 8));
        probe.observe_preparation((200, 20), (60, 6)).unwrap();
        assert!(probe.observe_nested((100, 10), (51, 5)).is_err());
        if arithmetic_failure {
            assert!(probe.observe_nested((100, 10), (usize::MAX, 0)).is_err());
        }
        let failure = probe.into_failure(ExactGeometryError::CapacityExceeded);
        assert_eq!(enclosing_failure_peak(&failure), Some((131, 13)));
        assert_eq!(
            (
                failure.admission_required_bytes(),
                failure.admission_required_items()
            ),
            (200, 20)
        );
        assert_eq!(is_enclosing_capacity_refusal(&failure), !arithmetic_failure);
        assert!(!is_configured_capacity_refusal(&failure));
        assert_eq!(failure.release(), &ExactGeometryRelease::default());
        assert_eq!(enclosing_failure_peak(&failure.clone()), Some((131, 13)));
    }
}

#[test]
fn preparation_maps_each_peak_without_discounting_configured_custody() {
    let mut probe =
        PreparationCapacityProbe::with_baselines((200, 20), (150, 15), (100, 10), (80, 8));
    probe.observe_preparation((200, 20), (40, 4)).unwrap();
    assert_eq!(probe.peaks(), ((200, 20), (140, 14)));
    probe.observe_preparation((170, 17), (0, 0)).unwrap();
    assert_eq!(probe.peaks(), ((200, 20), (150, 15)));
    probe.observe_preparation((100, 10), (0, 0)).unwrap();
    assert_eq!(probe.peaks(), ((200, 20), (150, 15)));

    for (configured, enclosing, configured_refusal) in [
        ((199, 20), (140, 14), true),
        ((200, 19), (140, 14), true),
        ((200, 20), (139, 14), false),
        ((200, 20), (140, 13), false),
        ((199, 20), (140, 13), true),
        ((200, 19), (139, 14), true),
    ] {
        let mut probe =
            PreparationCapacityProbe::with_baselines(configured, enclosing, (100, 10), (80, 8));
        assert_eq!(
            probe.observe_preparation((200, 20), (40, 4)),
            Err(ExactGeometryError::CapacityExceeded)
        );
        assert_eq!(probe.configured_refusal(), configured_refusal);
        assert_eq!(probe.enclosing_refusal(), !configured_refusal);
        assert_eq!(probe.peaks(), ((200, 20), (140, 14)));
        probe.observe_preparation((100, 10), (0, 0)).unwrap();
        assert!(!probe.configured_refusal());
        assert!(!probe.enclosing_refusal());
    }
}

#[test]
fn preparation_mapping_arithmetic_cannot_credit_existing_custody() {
    let maximum = (usize::MAX, usize::MAX);
    for (baseline, current, raw, credit) in [
        ((100, 10), (80, 8), (99, 10), (0, 0)),
        ((100, 10), (80, 8), (100, 9), (0, 0)),
        ((100, 10), (80, 8), (120, 12), (21, 0)),
        ((100, 10), (80, 8), (120, 12), (0, 3)),
        ((0, 0), (1, 0), (usize::MAX, 0), (1, 0)),
        ((0, 0), (0, 1), (0, usize::MAX), (0, 1)),
    ] {
        let mut probe =
            PreparationCapacityProbe::with_baselines(maximum, (0, 0), baseline, current);
        assert!(probe.observe((1, 1), (1, 1)).is_err());
        assert!(probe.enclosing_refusal());
        assert_eq!(
            probe.observe_preparation(raw, credit),
            Err(ExactGeometryError::CapacityExceeded)
        );
        assert!(!probe.configured_refusal());
        assert!(!probe.enclosing_refusal());
        assert_eq!(probe.peaks(), ((1, 1), (1, 1)));
    }
    let mut probe = PreparationCapacityProbe::with_baselines(maximum, maximum, maximum, maximum);
    probe.observe_preparation(maximum, (0, 0)).unwrap();
    assert_eq!(probe.peaks(), (maximum, maximum));
    let mut probe = PreparationCapacityProbe::with_baselines(maximum, (0, 0), (0, 0), (0, 0));
    probe.observe_preparation(maximum, maximum).unwrap();
    assert_eq!(probe.peaks(), (maximum, (0, 0)));
}

#[test]
fn nested_preparation_keeps_enclosing_baseline_and_prior_credit_peaks() {
    let mut probe =
        PreparationCapacityProbe::with_baselines((200, 20), (130, 13), (100, 10), (80, 8));
    probe.observe_preparation((200, 20), (60, 6)).unwrap();
    assert_eq!(probe.observe_nested((100, 10), (50, 5)).unwrap(), (150, 15));
    assert_eq!(probe.peaks(), ((200, 20), (130, 13)));
    for additional in [(51, 5), (50, 6)] {
        assert_eq!(
            probe.observe_nested((100, 10), additional),
            Err(ExactGeometryError::CapacityExceeded)
        );
        assert!(probe.enclosing_refusal());
        assert!(!probe.configured_refusal());
    }
    assert_eq!(probe.peaks(), ((200, 20), (131, 14)));
    assert_eq!(
        probe.observe_nested((99, 10), (0, 0)),
        Err(ExactGeometryError::CapacityExceeded)
    );
    assert!(!probe.enclosing_refusal());
    assert!(!probe.configured_refusal());
    assert_eq!(probe.peaks(), ((200, 20), (131, 14)));
    probe.observe_nested((100, 10), (0, 0)).unwrap();
}

#[test]
fn nested_preparation_preserves_observations_and_clears_overflow_attribution() {
    for (configured, enclosing, configured_refusal) in [
        ((149, 15), (150, 15), true),
        ((150, 14), (150, 15), true),
        ((149, 15), (150, 14), true),
        ((150, 15), (149, 15), false),
        ((150, 15), (150, 14), false),
    ] {
        let mut probe = PreparationCapacityProbe::new(configured, enclosing);
        probe.observe((120, 12), (110, 11)).unwrap();
        assert_eq!(
            probe.observe_nested((100, 10), (50, 5)),
            Err(ExactGeometryError::CapacityExceeded)
        );
        assert_eq!(probe.configured_refusal(), configured_refusal);
        assert_eq!(probe.enclosing_refusal(), !configured_refusal);
        assert_eq!(probe.peaks(), ((150, 15), (150, 15)));
        assert_eq!(
            probe.observe_nested((100, 10), (usize::MAX, 0)),
            Err(ExactGeometryError::CapacityExceeded)
        );
        assert!(!probe.configured_refusal());
        assert!(!probe.enclosing_refusal());
        assert_eq!(probe.peaks(), ((150, 15), (150, 15)));
        probe.observe_nested((100, 10), (0, 0)).unwrap();
        assert_eq!(probe.peaks(), ((150, 15), (150, 15)));
    }
    let mut probe = PreparationCapacityProbe::new((200, 20), (150, 15));
    probe.observe((200, 20), (140, 14)).unwrap();
    assert_eq!(probe.observe_nested((100, 10), (50, 5)).unwrap(), (150, 15));
    assert_eq!(probe.peaks(), ((200, 20), (150, 15)));
    let mut probe = PreparationCapacityProbe::new((0, 0), (0, 0));
    probe.observe_nested((0, 0), (0, 0)).unwrap();
}

#[test]
fn preparation_observes_configured_and_enclosing_peaks_independently() {
    let mut probe = PreparationCapacityProbe::new((200, 20), (150, 15));
    probe.observe((200, 10), (100, 8)).unwrap();
    probe.observe((180, 20), (150, 15)).unwrap();
    assert_eq!(probe.peaks(), ((200, 20), (150, 15)));
    // The smaller raw observation has the larger enclosing charge.
    probe.observe((50, 5), (40, 4)).unwrap();
    assert_eq!(probe.peaks(), ((200, 20), (150, 15)));
    assert!(!probe.configured_refusal());
    assert!(!probe.enclosing_refusal());

    for (configured, enclosing, configured_refusal) in [
        ((201, 10), (100, 8), true),
        ((100, 21), (100, 8), true),
        ((200, 20), (151, 15), false),
        ((200, 20), (150, 16), false),
        ((201, 20), (150, 16), true),
        ((200, 21), (151, 15), true),
    ] {
        let mut probe = PreparationCapacityProbe::new((200, 20), (150, 15));
        assert_eq!(
            probe.observe(configured, enclosing),
            Err(ExactGeometryError::CapacityExceeded)
        );
        assert_eq!(probe.configured_refusal(), configured_refusal);
        assert_eq!(probe.enclosing_refusal(), !configured_refusal);
        assert_eq!(probe.peaks(), (configured, enclosing));
        probe.observe((0, 0), (0, 0)).unwrap();
        assert!(!probe.configured_refusal());
        assert!(!probe.enclosing_refusal());
        assert_eq!(probe.peaks(), (configured, enclosing));
    }
    let mut probe = PreparationCapacityProbe::new((0, 0), (0, 0));
    probe.observe((0, 0), (0, 0)).unwrap();
    assert_eq!(
        probe.observe((0, 0), (1, 0)),
        Err(ExactGeometryError::CapacityExceeded)
    );
    assert!(probe.enclosing_refusal());
    let mut probe =
        PreparationCapacityProbe::new((usize::MAX, usize::MAX), (usize::MAX, usize::MAX));
    probe.observe((usize::MAX, 0), (0, usize::MAX)).unwrap();
    assert_eq!(probe.peaks(), ((usize::MAX, 0), (0, usize::MAX)));
}

#[test]
fn pre_shaping_reservation_requires_positive_capacity_in_both_dimensions() {
    for occupied in [(100, 10), (0, 0)] {
        let exact = (occupied.0 + 1, occupied.1 + 1);
        assert_eq!(
            preparation_remaining_capacity(occupied, exact, exact).unwrap(),
            (1, 1)
        );
        assert_eq!(
            preparation_remaining_capacity(occupied, (1000, 100), (500, 50)).unwrap(),
            (500 - occupied.0, 50 - occupied.1)
        );
        for enclosing in [(occupied.0, 100), (1000, occupied.1), occupied] {
            let failure =
                preparation_remaining_capacity(occupied, (1000, 100), enclosing).unwrap_err();
            assert!(is_enclosing_capacity_refusal(&failure));
            assert!(!is_configured_capacity_refusal(&failure));
            assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
            assert_eq!(failure.release(), &ExactGeometryRelease::default());
            assert_eq!(enclosing_failure_peak(&failure), Some(exact));
            assert_eq!(
                (
                    failure.admission_required_bytes(),
                    failure.admission_required_items()
                ),
                exact
            );
        }
        for (configured, enclosing) in [
            ((occupied.0, 100), (1000, occupied.1)),
            ((1000, occupied.1), (occupied.0, 100)),
        ] {
            let failure =
                preparation_remaining_capacity(occupied, configured, enclosing).unwrap_err();
            assert!(is_configured_capacity_refusal(&failure));
            assert!(!is_enclosing_capacity_refusal(&failure));
        }
    }
    for occupied in [(usize::MAX, 0), (0, usize::MAX), (usize::MAX, usize::MAX)] {
        let failure = preparation_remaining_capacity(
            occupied,
            (usize::MAX, usize::MAX),
            (usize::MAX, usize::MAX),
        )
        .unwrap_err();
        assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
        assert!(!is_configured_capacity_refusal(&failure));
        assert!(!is_enclosing_capacity_refusal(&failure));
        assert_eq!(failure.release(), &ExactGeometryRelease::default());
        assert_eq!(enclosing_failure_peak(&failure), Some((0, 0)));
    }
}

#[gpui::test]
fn immutable_response_preparation_obeys_enclosing_ceilings(cx: &mut TestAppContext) {
    with_text_system(cx, |text_system| {
        let source = "resident payload";
        for index in [false, true] {
            for resident in [false, true] {
                let make_owner = |bytes, items| {
                    owner_with_retained_items(source, 32, 240., 8, bytes, items, style()).unwrap()
                };
                let mut owner = make_owner(512 * 1024, 8192);
                let target = BlockTarget::new(px(0.), px(80.), px(32.));
                if !index {
                    let key = start_index(&mut owner, 80_000);
                    let page = page(&mut owner, key, source, 0, source.len(), 80_000);
                    assert_eq!(
                        admit_page_with_empty_objects(&mut owner, key, &page, text_system)
                            .unwrap()
                            .progress(),
                        ExactGeometryProgress::IndexComplete
                    );
                }
                let start = if index {
                    owner.start_index(GeometryJobId::new(90_000))
                } else {
                    owner.request_block_target(GeometryJobId::new(90_000), target)
                }
                .unwrap();
                let page = page(&mut owner, start.key(), source, 0, source.len(), 90_000);
                let ids = (
                    GeometryJobId::new(90_001),
                    PageRequestId::new(90_001),
                    ObjectRequestId::new(90_001),
                );
                let prepare =
                    |owner: &ExactGeometryOwner, objects: Option<&ObjectPage>, capacity| {
                        prepare_response(
                            owner,
                            start.key(),
                            &page,
                            objects,
                            text_system,
                            resident,
                            index,
                            ids,
                            target,
                            capacity,
                        )
                    };
                let prepared = prepare(&owner, None, (usize::MAX, usize::MAX)).unwrap();
                let (bytes, items) = prepared.required_capacity();
                assert_eq!(prepared.enclosing_peak(), Some((bytes, items)));
                if index {
                    for (bytes, items) in [(bytes - 1, 8192), (512 * 1024, items - 1)] {
                        let mut limited = make_owner(bytes, items);
                        limited.start_index(GeometryJobId::new(90_000)).unwrap();
                        limited
                            .request_page(start.key(), PageRequestId::new(90_000))
                            .unwrap();
                        let counts = limited.counts();
                        let failure =
                            prepare(&limited, None, (usize::MAX, usize::MAX)).unwrap_err();
                        assert_eq!(failure.error(), &ExactGeometryError::CapacityExceeded);
                        assert!(is_configured_capacity_refusal(&failure), "{failure:?}");
                        assert!(!is_enclosing_capacity_refusal(&failure));
                        assert_eq!(failure.release(), &ExactGeometryRelease::default());
                        assert_eq!(limited.counts(), counts);
                    }
                }
                if index && !resident {
                    let first = prepare(&owner, None, (0, 0)).unwrap_err();
                    let first_bytes = first.admission_required_bytes();
                    let first_items = first.admission_required_items();
                    for (configured, enclosing) in [
                        ((first_bytes - 1, 8192), (usize::MAX, first_items - 1)),
                        ((512 * 1024, first_items - 1), (first_bytes - 1, usize::MAX)),
                    ] {
                        let mut limited = make_owner(configured.0, configured.1);
                        limited.start_index(GeometryJobId::new(90_000)).unwrap();
                        limited
                            .request_page(start.key(), PageRequestId::new(90_000))
                            .unwrap();
                        let before = limited.counts();
                        let failure = prepare(&limited, None, enclosing).unwrap_err();
                        assert!(is_configured_capacity_refusal(&failure), "{failure:?}");
                        assert!(!is_enclosing_capacity_refusal(&failure));
                        assert_eq!(failure.release(), &ExactGeometryRelease::default());
                        assert_eq!(limited.counts(), before);
                    }
                }
                check_refusals(&owner, bytes, items, |capacity| {
                    prepare(&owner, None, capacity)
                });
                let request = prepared.object_request().unwrap();
                prepared.commit(&mut owner);
                let stale = prepare(&owner, None, (usize::MAX, usize::MAX)).unwrap_err();
                assert_eq!(enclosing_failure_peak(&stale), None);
                let objects = ObjectPage::new(
                    ObjectPageId::new(90_000),
                    request.key(),
                    vec![],
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    true,
                    None,
                )
                .unwrap();
                let prepared = prepare(&owner, Some(&objects), (usize::MAX, usize::MAX)).unwrap();
                assert_eq!(prepared.terminal_index(), index);
                assert!(index || prepared.terminal_target());
                let (bytes, items) = prepared.required_capacity();
                assert_eq!(prepared.enclosing_peak(), Some((bytes, items)));
                check_refusals(&owner, bytes, items, |capacity| {
                    prepare(&owner, Some(&objects), capacity)
                });
                prepared.commit(&mut owner);
            }
        }
    });
}

fn check_refusals(
    owner: &ExactGeometryOwner,
    bytes: usize,
    items: usize,
    prepare: impl Fn(
        (usize, usize),
    ) -> Result<
        gpui_text_input::preparation_test_support::PreparedResponseProbe,
        gpui_text_input::ExactGeometryFailure,
    >,
) {
    let counts = owner.counts();
    for (bytes, items, succeeds) in [
        (bytes, items, true),
        (bytes - 1, usize::MAX, false),
        (usize::MAX, items - 1, false),
        (0, usize::MAX, false),
        (usize::MAX, 0, false),
        (bytes, items, true),
    ] {
        let result = prepare((bytes, items));
        assert_eq!(result.is_ok(), succeeds, "{result:?}");
        if let Err(failure) = result {
            assert_eq!(failure.release(), &ExactGeometryRelease::default());
            assert_eq!(
                enclosing_failure_peak(&failure),
                Some((
                    failure.admission_required_bytes(),
                    failure.admission_required_items(),
                ))
            );
            if matches!(failure.error(), ExactGeometryError::Layout(_)) {
                assert!(!is_enclosing_capacity_refusal(&failure));
            } else {
                assert!(is_enclosing_capacity_refusal(&failure), "{failure:?}");
            }
            assert!(!is_configured_capacity_refusal(&failure));
        }
        assert_eq!(owner.counts(), counts);
    }
}
