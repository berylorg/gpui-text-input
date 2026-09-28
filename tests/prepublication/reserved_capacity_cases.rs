use super::*;

#[gpui::test]
fn availability_cannot_expand_either_reserved_dimension(cx: &mut TestAppContext) {
    let source = "reservation";
    let seed = seed(source, 1, 0);
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let mut config = config(source, 1, 32);
        config.limits.max_realization_work_per_frame = 1;
        let (environment, cleanup) = make_environment(62, config.clone(), window.text_system());
        let mut probe = RangePrepublicationSession::new(seed, environment).unwrap();
        assert_eq!(probe.service(window.text_system()).effects.len(), 1);
        let required = probe.high_water();
        drop(probe);
        let _ = drain_cleanup(&cleanup);

        for (ceiling, admitted) in [
            (required, true),
            (
                RangeSurfaceCharge {
                    bytes: required.bytes - 1,
                    ..required
                },
                false,
            ),
            (
                RangeSurfaceCharge {
                    items: required.items - 1,
                    ..required
                },
                false,
            ),
        ] {
            let (environment, cleanup) = make_environment(63, config.clone(), window.text_system());
            let mut session =
                RangePrepublicationSession::new_with_admission_capacity(seed, environment, ceiling)
                    .unwrap();
            session.set_available_capacity(RangeSurfaceCharge {
                bytes: usize::MAX,
                items: usize::MAX,
            });
            let step = session.service(window.text_system());
            assert_eq!(step.effects.len(), usize::from(admitted));
            if !admitted {
                assert_eq!(step.status, RangePrepublicationStatus::CapacityBlocked);
            }
            drop(session);
            let _ = drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
        }
    });
}

#[gpui::test]
fn initial_reservation_rejects_byte_and_item_shortfalls_without_effects(cx: &mut TestAppContext) {
    let source = "reserved source";
    let seed = seed(source, 1, 3);
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let (environment, cleanup) =
            make_environment(60, config(source, 1, 32), window.text_system());
        let probe = RangePrepublicationSession::new(seed, environment.clone()).unwrap();
        let ownership = probe.ownership();
        let exact = RangeSurfaceCharge {
            bytes: ownership.bytes,
            items: ownership.items,
        };
        drop(probe);
        for capacity in [
            RangeSurfaceCharge {
                bytes: exact.bytes - 1,
                ..exact
            },
            RangeSurfaceCharge {
                items: exact.items - 1,
                ..exact
            },
            RangeSurfaceCharge::default(),
        ] {
            assert_eq!(
                RangePrepublicationSession::new_with_admission_capacity(
                    seed,
                    environment.clone(),
                    capacity
                )
                .err()
                .unwrap(),
                RangePrepublicationFailure::InitialCapacityDenied,
            );
            assert_eq!(cleanup.ownership().active, 0);
        }
        let mut session =
            RangePrepublicationSession::new_with_admission_capacity(seed, environment, exact)
                .unwrap();
        for available in [
            RangeSurfaceCharge::default(),
            exact,
            RangeSurfaceCharge {
                bytes: usize::MAX,
                items: usize::MAX,
            },
        ] {
            session.set_available_capacity(available);
            let step = session.service(window.text_system());
            assert_eq!(step.status, RangePrepublicationStatus::CapacityBlocked);
            assert!(step.effects.is_empty());
            assert_eq!(session.ownership().bytes, exact.bytes);
            assert_eq!(session.ownership().items, exact.items);
            assert_eq!(cleanup.ownership().active, 0);
        }
        drop(session);
        assert!(drain_cleanup(&cleanup).is_empty());
    });
}

#[gpui::test]
fn reserved_preparation_can_resume_within_ceiling_and_release_candidate(cx: &mut TestAppContext) {
    let source = "first line\nsecond line\nthird line";
    let seed = seed(source, 2, 15);
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let config = config(source, 2, 32);
        let ceiling = RangeSurfaceCharge {
            bytes: config.limits.max_surface_bytes / 2,
            items: config.limits.max_surface_items / 2,
        };
        let (environment, cleanup) = make_environment(61, config, window.text_system());
        let mut session =
            RangePrepublicationSession::new_with_admission_capacity(seed, environment, ceiling)
                .unwrap();
        session.set_available_capacity(RangeSurfaceCharge::default());
        let blocked = session.service(window.text_system());
        assert_eq!(blocked.status, RangePrepublicationStatus::CapacityBlocked);
        assert!(blocked.effects.is_empty());
        session.set_available_capacity(ceiling);
        let (candidate, _, _) = drive(&mut session, source, window.text_system(), &cleanup);
        assert!(session.high_water().bytes <= ceiling.bytes);
        assert!(session.high_water().items <= ceiling.items);
        assert!(candidate.adoption_peak().bytes <= ceiling.bytes);
        assert!(candidate.adoption_peak().items <= ceiling.items);
        drop(candidate);
        drop(session);
        let released = drain_cleanup(&cleanup);
        assert!(released.iter().any(|effect| matches!(
            effect,
            RangePrepublicationCleanupEffect::ReleaseCandidate { .. }
        )));
        assert_eq!(cleanup.ownership().active, 0);
    });
}
