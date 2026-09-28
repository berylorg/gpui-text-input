use super::*;
use gpui_text_input::preparation_test_support::session_delivered_text_preparation;

const SOURCE: &str =
    "first line of text spanning pages\nsecond line of text spanning pages\nthird line";

fn replacement_config() -> RangeTextInputConfig {
    let mut config = config(SOURCE, 1, 32);
    config.limits.max_realization_work_per_frame = 1;
    config.residency_limits = ResidencyLimits::new(1, 1024 * 1024, 4, 128).unwrap();
    config
}

fn reach_replacement(
    session: &mut RangePrepublicationSession,
    text_system: &Arc<WindowTextSystem>,
    cleanup: &RangePrepublicationCleanupLedger,
) {
    for id in 1..300 {
        let step = session.service(text_system);
        assert!(
            !matches!(step.status, RangePrepublicationStatus::Failed(_)),
            "{:?}",
            step.status
        );
        for effect in step.effects {
            let replacement = matches!(effect,
                RangePrepublicationEffect::Page { request, .. }
                if request.key().purpose() == PagePurpose::GeometryIndex
                    && session.ownership().resident_pages == 1);
            assert_eq!(
                super::admitted_response::deliver(session, SOURCE, id, &effect),
                RangePrepublicationDelivery::Accepted
            );
            if replacement {
                return;
            }
        }
        let _ = drain_cleanup(cleanup);
    }
    panic!("adjacent replacement response not reached");
}

#[gpui::test]
fn delivered_text_replacement_preserves_custody_until_storage_fits(cx: &mut TestAppContext) {
    let window = cx.add_empty_window();
    window.update(|window, _| {
        for cancel in [false, true] {
            let (environment, cleanup) =
                make_environment(96, replacement_config(), window.text_system());
            let mut session =
                RangePrepublicationSession::new(seed(SOURCE, 1, 0), environment).unwrap();
            reach_replacement(&mut session, window.text_system(), &cleanup);
            let proof = session_delivered_text_preparation(&session).unwrap();
            let (_, _, current, exact) = proof;
            assert!(exact.bytes > current.bytes);
            assert!(exact.items > current.items);
            let before = session.ownership();
            let records = cleanup.ownership().active;
            for available in [
                RangeSurfaceCharge { bytes: 0, items: 0 },
                current,
                RangeSurfaceCharge {
                    bytes: exact.bytes - 1,
                    ..exact
                },
                RangeSurfaceCharge {
                    items: exact.items - 1,
                    ..exact
                },
            ] {
                session.set_available_capacity(available);
                for _ in 0..2 {
                    let step = session.service(window.text_system());
                    assert_eq!(step.status, RangePrepublicationStatus::CapacityBlocked);
                    assert_eq!(step.spent, 0);
                    assert!(step.effects.is_empty());
                    assert_eq!(session.ownership(), before);
                    assert_eq!(session_delivered_text_preparation(&session), Some(proof));
                    assert_eq!(cleanup.ownership().active, records);
                    assert!(drain_cleanup(&cleanup).is_empty());
                    assert!(session.high_water().bytes >= exact.bytes);
                    assert!(session.high_water().items >= exact.items);
                }
            }
            if !cancel {
                session.set_available_capacity(exact);
                let step = session.service(window.text_system());
                assert_eq!(step.status, RangePrepublicationStatus::Advancing);
                assert_eq!(step.spent, 1);
                assert!(step.effects.is_empty());
                assert_eq!(session.ownership().resident_pages, 1);
                assert!(session_delivered_text_preparation(&session).is_none());
                let _ = drain_cleanup(&cleanup);
                session.set_available_capacity(RangeSurfaceCharge {
                    bytes: usize::MAX,
                    items: usize::MAX,
                });
                let (candidate, _, _) = drive(&mut session, SOURCE, window.text_system(), &cleanup);
                drop(candidate);
            }
            drop(session);
            let _ = drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
        }
    });
}

#[gpui::test]
fn delivered_text_residency_failure_precedes_zero_host_capacity(cx: &mut TestAppContext) {
    let window = cx.add_empty_window();
    window.update(|window, _| {
        let mut config = replacement_config();
        config.residency_limits = ResidencyLimits::new(1, 31, 4, 128).unwrap();
        let (environment, cleanup) = make_environment(97, config, window.text_system());
        let mut session = RangePrepublicationSession::new(seed(SOURCE, 1, 0), environment).unwrap();
        let mut delivered = false;
        for id in 1..20 {
            let step = session.service(window.text_system());
            for effect in step.effects {
                assert_eq!(
                    super::admitted_response::deliver(&mut session, SOURCE, id, &effect),
                    RangePrepublicationDelivery::Accepted
                );
                delivered = matches!(effect, RangePrepublicationEffect::Page { .. });
            }
            if delivered {
                break;
            }
        }
        assert!(delivered);
        session.set_available_capacity(RangeSurfaceCharge { bytes: 0, items: 0 });
        let step = session.service(window.text_system());
        assert_eq!(
            step.status,
            RangePrepublicationStatus::Failed(RangePrepublicationFailure::TerminalCapacity)
        );
        assert!(step.effects.is_empty());
        drop(session);
        let _ = drain_cleanup(&cleanup);
        assert_eq!(cleanup.ownership().active, 0);
    });
}
