use super::*;
use gpui_text_input::preparation_test_support::{
    session_initial_index_identity, session_initial_index_peak,
};

#[gpui::test]
fn initial_index_configured_shortfalls_remain_terminal_with_zero_host_capacity(
    cx: &mut TestAppContext,
) {
    let source = "restored text";
    let window = cx.add_empty_window();
    window.update(|window, _| {
        for byte_limit in [false, true] {
            let mut config = config(source, 1, 32);
            config.limits.max_realization_work_per_frame = 1;
            let (bytes, items) = gpui_text_input::ExactGeometryOwner::initial_required_charge(
                &config.layout,
                &config.style.clone(),
            )
            .unwrap();
            config.geometry_limits = ExactGeometryLimits::new(
                32,
                16,
                if byte_limit { bytes } else { 2 * 1024 * 1024 },
                if byte_limit { 32_768 } else { items },
            )
            .unwrap();
            let (environment, cleanup) = make_environment(95, config, window.text_system());
            let mut session =
                RangePrepublicationSession::new(seed(source, 1, 3), environment).unwrap();
            reach_initial_index(&mut session, source, window.text_system(), &cleanup);
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
        }
    });
}

fn reach_initial_index(
    session: &mut RangePrepublicationSession,
    source: &str,
    text_system: &Arc<WindowTextSystem>,
    cleanup: &RangePrepublicationCleanupLedger,
) -> u64 {
    for id in 1..200 {
        if let Some(identity) = session_initial_index_identity(session) {
            return identity;
        }
        let step = session.service(text_system);
        assert!(!matches!(step.status, RangePrepublicationStatus::Failed(_)));
        for effect in step.effects {
            assert_eq!(
                super::admitted_response::deliver(session, source, id, &effect),
                RangePrepublicationDelivery::Accepted,
            );
        }
        let _ = drain_cleanup(cleanup);
    }
    panic!("initial index boundary was not reached");
}

#[gpui::test]
fn initial_index_capacity_refusal_preserves_identity_custody_and_retry(cx: &mut TestAppContext) {
    let source = "first line\nsecond line\nthird line";
    let window = cx.add_empty_window();
    window.update(|window, _| {
        for cancel in [false, true] {
            let mut config = config(source, 1, 32);
            config.limits.max_realization_work_per_frame = 1;
            let (environment, cleanup) = make_environment(94, config, window.text_system());
            let mut session =
                RangePrepublicationSession::new(seed(source, 1, 7), environment).unwrap();
            let identity =
                reach_initial_index(&mut session, source, window.text_system(), &cleanup);
            let before = session.ownership();
            assert!(before.resident_pages > 0);
            assert!(before.resident_object_pages > 0);
            let records = cleanup.ownership().active;
            let exact = session_initial_index_peak(&session).unwrap();
            assert!(exact.bytes > before.bytes);
            assert!(exact.items > before.items);
            for available in [
                RangeSurfaceCharge { bytes: 0, items: 0 },
                RangeSurfaceCharge {
                    bytes: before.bytes,
                    items: usize::MAX,
                },
                RangeSurfaceCharge {
                    bytes: usize::MAX,
                    items: before.items,
                },
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
                    assert_eq!(session_initial_index_identity(&session), Some(identity));
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
                assert_eq!(session_initial_index_identity(&session), None);
                assert_eq!(session.ownership().resident_pages, 0);
                assert_eq!(session.ownership().resident_object_pages, 0);
                let _ = drain_cleanup(&cleanup);
                session.set_available_capacity(RangeSurfaceCharge {
                    bytes: usize::MAX,
                    items: usize::MAX,
                });
                let (candidate, _, _) = drive(&mut session, source, window.text_system(), &cleanup);
                drop(candidate);
            }
            drop(session);
            let _ = drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
        }
    });
}
