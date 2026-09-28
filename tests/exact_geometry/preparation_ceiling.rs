use super::*;
use gpui_text_input::preparation_test_support::{
    is_configured_capacity_refusal, is_enclosing_capacity_refusal, preparation_remaining_capacity,
    prepare_response,
};

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
