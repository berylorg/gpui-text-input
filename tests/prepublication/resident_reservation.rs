use super::*;
use gpui::Focusable;

fn add(left: RangeSurfaceCharge, right: RangeSurfaceCharge) -> RangeSurfaceCharge {
    RangeSurfaceCharge {
        bytes: left.bytes + right.bytes,
        items: left.items + right.items,
    }
}

fn mounted(
    source: &str,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> (
    gpui::Entity<RangeTextInput>,
    RangePrepublicationCleanupLedger,
) {
    let mut restoration = seed(source, 1, 8);
    restoration.selection.anchor = position(10);
    let (environment, cleanup) = make_environment(101, config(source, 1, 32), window.text_system());
    let mut session = RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
    let (candidate, _, _) = drive(&mut session, source, window.text_system(), &cleanup);
    let current = RangePrepublicationCurrent {
        binding: restoration.binding,
        history: restoration.history,
        available_capacity: candidate.adoption_peak(),
    };
    let input = cx.new(|cx| {
        RangeTextInput::new_with_prepublication(&environment, candidate, current, window, cx)
            .unwrap()
    });
    (input, cleanup)
}

#[gpui::test]
fn combined_reservation_admits_exact_initial_fit_and_rejects_shortfalls(cx: &mut TestAppContext) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let window = cx.add_empty_window();
    let (input, old_cleanup) = window.update(|window, cx| mounted(source, window, cx));
    window.update(|window, cx| {
        let mut successor = seed(source, 2, 8);
        successor.selection.anchor = position(10);
        let (environment, cleanup) =
            make_environment(102, config(source, 2, 32), window.text_system());
        let probe = RangePrepublicationSession::new(successor, environment.clone()).unwrap();
        let initial = probe.ownership();
        let initial = RangeSurfaceCharge {
            bytes: initial.bytes,
            items: initial.items,
        };
        drop(probe);
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            let protection = input.protect_resident(cx).unwrap();
            let before = input.realization_diagnostics();
            let old = RangeSurfaceCharge {
                bytes: before.current.owned_bytes,
                items: before.current.owned_items,
            };
            let exact = add(old, initial);
            for available in [
                RangeSurfaceCharge {
                    bytes: exact.bytes - 1,
                    ..exact
                },
                RangeSurfaceCharge {
                    items: exact.items - 1,
                    ..exact
                },
                RangeSurfaceCharge {
                    bytes: old.bytes - 1,
                    ..exact
                },
                RangeSurfaceCharge {
                    items: old.items - 1,
                    ..exact
                },
            ] {
                assert!(matches!(
                    input.prepare_resident_successor(
                        protection,
                        successor,
                        environment.clone(),
                        available
                    ),
                    Err(RangePrepublicationFailure::InitialCapacityDenied)
                ));
                assert_eq!(cleanup.ownership().active, 0);
                assert_eq!(input.realization_diagnostics().current, before.current);
                assert!(input.surface().is_some());
                assert!(input.take_request().is_none());
            }
            let (mut session, reservation) = input
                .prepare_resident_successor(protection, successor, environment.clone(), exact)
                .unwrap();
            assert_eq!(reservation.predecessor_charge(), old);
            assert_eq!(reservation.combined_capacity(), exact);
            assert_eq!(reservation.successor_capacity(), initial);
            assert_eq!(reservation.protection(), protection);
            assert_eq!(reservation.session_generation(), session.generation());
            session.set_available_capacity(RangeSurfaceCharge {
                bytes: usize::MAX,
                items: usize::MAX,
            });
            let step = session.service(window.text_system());
            assert_eq!(step.status, RangePrepublicationStatus::CapacityBlocked);
            assert!(step.effects.is_empty());
            drop(session);
            drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
            assert_eq!(input.realization_diagnostics().current, before.current);
            input.release_resident_protection(protection, cx).unwrap();
        });
    });
    drop(input);
    window.update(|_, _| {});
    cx.run_until_parked();
    drain_cleanup(&old_cleanup);
    assert_eq!(old_cleanup.ownership().active, 0);
}

#[gpui::test]
fn combined_reservation_keeps_predecessor_paint_and_drains_cancelled_successors(
    cx: &mut TestAppContext,
) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let window = cx.add_empty_window();
    let (input, old_cleanup) = window.update(|window, cx| mounted(source, window, cx));
    window.update(|window, cx| {
        let mut successor = seed(source, 2, 8);
        successor.selection.anchor = position(10);
        let (environment, cleanup) =
            make_environment(103, config(source, 2, 32), window.text_system());
        let mut probe = RangePrepublicationSession::new(successor, environment.clone()).unwrap();
        let (candidate, _, _) = drive(&mut probe, source, window.text_system(), &cleanup);
        let peak = probe.high_water();
        drop(candidate);
        drop(probe);
        drain_cleanup(&cleanup);
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            let protection = input.protect_resident(cx).unwrap();
            let old_seed = protection.seed();
            let focus = input.focus_handle(cx);
            let before = input.realization_diagnostics().current;
            let combined = add(
                RangeSurfaceCharge {
                    bytes: before.owned_bytes,
                    items: before.owned_items,
                },
                peak,
            );
            let mut previous_generation = None;
            for cancel_early in [true, false, true, false] {
                let (mut session, reservation) = input
                    .prepare_resident_successor(
                        protection,
                        successor,
                        environment.clone(),
                        combined,
                    )
                    .unwrap();
                assert_ne!(previous_generation, Some(reservation.session_generation()));
                previous_generation = Some(reservation.session_generation());
                assert_eq!(reservation.successor_capacity(), peak);
                if cancel_early {
                    let step = session.service(window.text_system());
                    assert!(!step.effects.is_empty());
                    session.cancel();
                } else {
                    let (candidate, _, _) =
                        drive(&mut session, source, window.text_system(), &cleanup);
                    assert_eq!(candidate.generation(), reservation.session_generation());
                    assert!(session.high_water().bytes <= peak.bytes);
                    assert!(session.high_water().items <= peak.items);
                    assert_eq!(candidate.source_binding(), successor.binding);
                    drop(candidate);
                }
                drop(session);
                drain_cleanup(&cleanup);
                assert_eq!(cleanup.ownership().active, 0);
                assert_eq!(cleanup.ownership().ready, 0);
                assert_eq!(cleanup.ownership().awaiting_acknowledgement, 0);
                assert!(input.resident_protection_is_current(protection));
                assert_eq!(
                    input.export_restoration(old_seed.history).unwrap(),
                    old_seed
                );
                assert_eq!(input.realization_diagnostics().current, before);
                assert_eq!(input.focus_handle(cx), focus);
                assert!(input.surface().is_some());
                assert!(!input.is_enabled());
                assert!(input.take_request().is_none());
            }
            input.release_resident_protection(protection, cx).unwrap();
            assert!(matches!(
                input.prepare_resident_successor(protection, successor, environment, combined),
                Err(RangePrepublicationFailure::Stale)
            ));
        });
    });
    drop(input);
    window.update(|_, _| {});
    cx.run_until_parked();
    drain_cleanup(&old_cleanup);
    assert_eq!(old_cleanup.ownership().active, 0);
}

#[gpui::test]
fn combined_reservation_rejects_changed_positions_and_stale_cuts(cx: &mut TestAppContext) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let window = cx.add_empty_window();
    let (input, old_cleanup) = window.update(|window, cx| mounted(source, window, cx));
    let (other, other_cleanup) = window.update(|window, cx| mounted(source, window, cx));
    window.update(|window, cx| {
        let (environment, cleanup) =
            make_environment(104, config(source, 2, 32), window.text_system());
        let generous = RangeSurfaceCharge {
            bytes: usize::MAX,
            items: usize::MAX,
        };
        let mut successor = seed(source, 2, 8);
        successor.selection.anchor = position(10);
        let foreign = other.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            input.protect_resident(cx).unwrap()
        });
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            let cut = input.protect_resident(cx).unwrap();
            let before = input.realization_diagnostics().current;
            assert!(matches!(
                input.prepare_resident_successor(foreign, successor, environment.clone(), generous),
                Err(RangePrepublicationFailure::Stale)
            ));
            let mut moved_caret = successor;
            moved_caret.caret = position(9);
            moved_caret.selection.head = position(9);
            let mut moved_anchor = successor;
            moved_anchor.selection.anchor = position(11);
            let mut moved_scroll = successor;
            moved_scroll.scroll.position = position(9);
            for changed in [moved_caret, moved_anchor, moved_scroll] {
                assert!(matches!(
                    input.prepare_resident_successor(cut, changed, environment.clone(), generous),
                    Err(RangePrepublicationFailure::SourceMismatch)
                ));
                assert_eq!(cleanup.ownership().active, 0);
                assert_eq!(input.realization_diagnostics().current, before);
            }
            let (session, reservation) = input
                .prepare_resident_successor(cut, successor, environment.clone(), generous)
                .unwrap();
            assert_eq!(
                reservation.successor_capacity(),
                RangeSurfaceCharge {
                    bytes: environment.config().limits.max_surface_bytes,
                    items: environment.config().limits.max_surface_items,
                }
            );
            drop(session);
            input.release_resident_protection(cut, cx).unwrap();
            let current_cut = input.protect_resident(cx).unwrap();
            assert!(matches!(
                input.prepare_resident_successor(cut, successor, environment.clone(), generous),
                Err(RangePrepublicationFailure::Stale)
            ));
            let layout = environment.config().layout.clone();
            assert!(
                input
                    .set_layout(layout, environment.config().style.clone(), cx)
                    .is_err()
            );
            assert!(matches!(
                input.prepare_resident_successor(
                    current_cut,
                    successor,
                    environment.clone(),
                    generous
                ),
                Err(RangePrepublicationFailure::Stale)
            ));
            assert_eq!(input.realization_diagnostics().current, before);
            assert!(input.surface().is_some());
            assert!(input.take_request().is_none());
            assert_eq!(cleanup.ownership().active, 0);
            input.release_resident_protection(current_cut, cx).unwrap();
        });
    });
    drop(input);
    drop(other);
    window.update(|_, _| {});
    cx.run_until_parked();
    drain_cleanup(&old_cleanup);
    drain_cleanup(&other_cleanup);
    assert_eq!(old_cleanup.ownership().active, 0);
    assert_eq!(other_cleanup.ownership().active, 0);
}
