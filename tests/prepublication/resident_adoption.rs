use super::*;
use gpui::Focusable;
use gpui_text_input::{
    RangePrepublicationCandidate, RangeResidentProtection, RangeResidentReservation,
    RangeTextInputEvent,
};

#[gpui::test]
fn resident_adoption_rejects_candidate_from_another_window(cx: &mut TestAppContext) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let original = seed(source, 1, 8);
    let (input, old_cleanup) = {
        let window = cx.add_empty_window();
        window.update(|window, cx| mounted(source, original, &[], window, cx))
    };
    let other = cx.add_empty_window();
    other.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            let cut = input.protect_resident(cx).unwrap();
            let (environment, cleanup) =
                make_environment(205, config(source, 2, 32), window.text_system());
            let successor = seed(source, 2, 8);
            let (reservation, candidate) =
                ready(input, cut, source, successor, &environment, &[], window);
            let current = RangePrepublicationCurrent {
                binding: successor.binding,
                history: successor.history,
                available_capacity: candidate.adoption_peak(),
            };
            assert_eq!(
                input.adopt_resident_successor(
                    reservation,
                    &environment,
                    candidate,
                    current,
                    window,
                    cx
                ),
                Err(RangePrepublicationAdoptionError::EnvironmentMismatch)
            );
            assert_eq!(
                input.export_restoration(original.history).unwrap(),
                original
            );
            drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
        })
    });
    drop(input);
    other.update(|_, _| {});
    cx.run_until_parked();
    drain_cleanup(&old_cleanup);
}

#[gpui::test]
fn resident_adoption_uses_exact_combined_reservation(cx: &mut TestAppContext) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let original = seed(source, 1, 8);
    let window = cx.add_empty_window();
    let (input, old_cleanup) =
        window.update(|window, cx| mounted(source, original, &[], window, cx));
    window.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            let cut = input.protect_resident(cx).unwrap();
            let successor = seed(source, 2, 8);
            let (environment, cleanup) =
                make_environment(206, config(source, 2, 32), window.text_system());
            let mut probe =
                RangePrepublicationSession::new(successor, environment.clone()).unwrap();
            let (candidate, _, _) = drive(&mut probe, source, window.text_system(), &cleanup);
            let peak = probe.high_water();
            drop(candidate);
            drop(probe);
            drain_cleanup(&cleanup);
            let old = input.realization_diagnostics().current;
            let combined = RangeSurfaceCharge {
                bytes: old.owned_bytes + peak.bytes,
                items: old.owned_items + peak.items,
            };
            let (mut session, reservation) = input
                .prepare_resident_successor(cut, successor, environment.clone(), combined)
                .unwrap();
            let (candidate, _, _) = drive(&mut session, source, window.text_system(), &cleanup);
            assert_eq!(reservation.successor_capacity(), peak);
            let current = RangePrepublicationCurrent {
                binding: successor.binding,
                history: successor.history,
                available_capacity: candidate.adoption_peak(),
            };
            input
                .adopt_resident_successor(reservation, &environment, candidate, current, window, cx)
                .unwrap();
            assert_eq!(
                input.export_restoration(successor.history).unwrap(),
                successor
            );
            assert!(input.take_request().is_none());
            assert!(input.dispose(window, cx).is_empty());
            drop(session);
            drain_cleanup(&cleanup);
            drain_cleanup(&old_cleanup);
            assert_eq!(cleanup.ownership().active, 0);
            assert_eq!(old_cleanup.ownership().active, 0);
        })
    });
}

fn mounted(
    source: &str,
    restoration: RangeRestorationSeed,
    objects: &[InlineObjectFact],
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> (
    gpui::Entity<RangeTextInput>,
    RangePrepublicationCleanupLedger,
) {
    let mut configuration = config(source, 1, 32);
    configuration.binding = restoration.binding;
    let (environment, cleanup) = make_environment(201, configuration, window.text_system());
    let mut session = RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
    let (candidate, _, _) = drive_with_objects(
        &mut session,
        source,
        window.text_system(),
        &cleanup,
        objects,
    );
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

fn ready(
    input: &RangeTextInput,
    cut: RangeResidentProtection,
    source: &str,
    seed: RangeRestorationSeed,
    environment: &RangePrepublicationEnvironment,
    objects: &[InlineObjectFact],
    window: &gpui::Window,
) -> (RangeResidentReservation, RangePrepublicationCandidate) {
    let (mut session, reservation) = input
        .prepare_resident_successor(
            cut,
            seed,
            environment.clone(),
            RangeSurfaceCharge {
                bytes: usize::MAX,
                items: usize::MAX,
            },
        )
        .unwrap();
    let (candidate, _, _) = drive_with_objects(
        &mut session,
        source,
        window.text_system(),
        environment.cleanup(),
        objects,
    );
    (reservation, candidate)
}

#[gpui::test]
fn resident_adoption_preserves_positions_focus_events_and_exact_cleanup(cx: &mut TestAppContext) {
    let source = "alpha\nbeta\ngamma\ndelta";
    for with_objects in [false, true] {
        let window = cx.add_empty_window();
        let objects = if with_objects {
            vec![object_fact(91, 8, 1)]
        } else {
            vec![]
        };
        let mut original = seed(source, 1, 8);
        if let Some(object) = objects.first() {
            let gap =
                InlineObjectGap::before(InlineObjectNeighbor::new(object.id(), object.order()));
            original.caret = SourcePosition::new(ByteOffset::new(8), gap);
            original.selection.head = original.caret;
            original.scroll.position = original.caret;
        }
        original.selection.anchor = position(10);
        original.scroll.intra_anchor = px(2.);
        let (input, old_cleanup) =
            window.update(|window, cx| mounted(source, original, &objects, window, cx));
        let events = Rc::new(Cell::new(0));
        let observed = events.clone();
        let subscription = window.update(|_, cx| {
            cx.subscribe(&input, move |_, _: &RangeTextInputEvent, _| {
                observed.set(observed.get() + 1)
            })
        });
        let successor_cleanup = window.update(|window, cx| {
            input.update(cx, |input, cx| {
                let focus = input.focus_handle(cx);
                window.focus(&focus);
                input.set_enabled(false, cx);
                let cut = input.protect_resident(cx).unwrap();
                let old_active = old_cleanup.ownership().active;
                assert!(old_active > 0);
                let mut successor = original;
                successor.binding = binding(source, 2);
                successor.history.as_mut().unwrap().binding = successor.binding;
                successor.history.as_mut().unwrap().id = 99;
                let (environment, cleanup) =
                    make_environment(202, config(source, 2, 32), window.text_system());
                let (reservation, candidate) = ready(
                    input,
                    cut,
                    source,
                    successor,
                    &environment,
                    &objects,
                    window,
                );
                let current = RangePrepublicationCurrent {
                    binding: successor.binding,
                    history: successor.history,
                    available_capacity: candidate.adoption_peak(),
                };
                input
                    .adopt_resident_successor(
                        reservation,
                        &environment,
                        candidate,
                        current,
                        window,
                        cx,
                    )
                    .unwrap();
                assert_eq!(
                    input.export_restoration(successor.history).unwrap(),
                    successor
                );
                assert_eq!(input.history_frontier(), successor.history.unwrap());
                assert_eq!(input.focus_handle(cx), focus);
                assert!(focus.is_focused(window));
                assert!(!input.is_enabled());
                assert!(!input.resident_protection_is_current(cut));
                input.set_enabled(true, cx);
                assert!(!input.is_enabled());
                assert!(input.take_request().is_none());
                assert_eq!(old_cleanup.ownership().ready, old_active);
                let released = drain_cleanup(&old_cleanup);
                assert_eq!(released.len(), old_active);
                assert!(drain_cleanup(&old_cleanup).is_empty());
                assert!(cleanup.ownership().active > 0);
                assert_eq!(cleanup.ownership().ready, 0);
                input.release_resident_protection(cut, cx).unwrap();
                assert!(!input.is_enabled());
                cleanup
            })
        });
        window.update(|_, _| {});
        assert_eq!(events.get(), 0);
        drop(subscription);
        drop(input);
        window.update(|_, _| {});
        cx.run_until_parked();
        drain_cleanup(&successor_cleanup);
        assert_eq!(successor_cleanup.ownership().active, 0);
        assert_eq!(old_cleanup.ownership().active, 0);
    }
}

#[gpui::test]
fn resident_adoption_refusals_preserve_paint_and_drain_candidate_custody(cx: &mut TestAppContext) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let original = seed(source, 1, 8);
    for refusal in 0..7 {
        let window = cx.add_empty_window();
        let (input, old_cleanup) =
            window.update(|window, cx| mounted(source, original, &[], window, cx));
        window.update(|window, cx| {
            input.update(cx, |input, cx| {
                input.set_enabled(false, cx);
                let cut = input.protect_resident(cx).unwrap();
                let successor = seed(source, 2, 8);
                let (environment, cleanup) =
                    make_environment(203, config(source, 2, 32), window.text_system());
                let (reservation, candidate) =
                    ready(input, cut, source, successor, &environment, &[], window);
                let mut current = RangePrepublicationCurrent {
                    binding: successor.binding,
                    history: successor.history,
                    available_capacity: candidate.adoption_peak(),
                };
                let before = input.realization_diagnostics().current;
                let old_active = old_cleanup.ownership().active;
                let focus = input.focus_handle(cx);
                let mut replacement = None;
                let expected = match refusal {
                    0 => {
                        current.binding = original.binding;
                        RangePrepublicationAdoptionError::SourceMismatch
                    }
                    1 => {
                        current.history = None;
                        RangePrepublicationAdoptionError::HistoryMismatch
                    }
                    2 => {
                        current.available_capacity.bytes -= 1;
                        RangePrepublicationAdoptionError::CapacityMismatch
                    }
                    3 => {
                        current.available_capacity.items -= 1;
                        RangePrepublicationAdoptionError::CapacityMismatch
                    }
                    4 => {
                        replacement = Some(
                            make_environment(203, config(source, 2, 32), window.text_system()).0,
                        );
                        RangePrepublicationAdoptionError::EnvironmentMismatch
                    }
                    5 => {
                        input.release_resident_protection(cut, cx).unwrap();
                        let _ = input.protect_resident(cx).unwrap();
                        RangePrepublicationAdoptionError::PredecessorMismatch
                    }
                    _ => {
                        assert!(
                            input
                                .set_layout(
                                    environment.config().layout.clone(),
                                    environment.config().style.clone(),
                                    cx
                                )
                                .is_err()
                        );
                        RangePrepublicationAdoptionError::PredecessorMismatch
                    }
                };
                let result = input.adopt_resident_successor(
                    reservation,
                    replacement.as_ref().unwrap_or(&environment),
                    candidate,
                    current,
                    window,
                    cx,
                );
                assert_eq!(result, Err(expected));
                assert_eq!(
                    input.export_restoration(original.history).unwrap(),
                    original
                );
                assert_eq!(input.realization_diagnostics().current, before);
                assert_eq!(input.focus_handle(cx), focus);
                assert!(!input.is_enabled());
                assert!(input.take_request().is_none());
                assert_eq!(old_cleanup.ownership().active, old_active);
                assert_eq!(old_cleanup.ownership().ready, 0);
                drain_cleanup(&cleanup);
                assert_eq!(cleanup.ownership().active, 0);
            })
        });
        drop(input);
        window.update(|_, _| {});
        cx.run_until_parked();
        drain_cleanup(&old_cleanup);
        assert_eq!(old_cleanup.ownership().active, 0);
    }
}

#[gpui::test]
fn resident_adoption_rejects_foreign_candidate_generation(cx: &mut TestAppContext) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let original = seed(source, 1, 8);
    let window = cx.add_empty_window();
    let (input, old_cleanup) =
        window.update(|window, cx| mounted(source, original, &[], window, cx));
    window.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            let cut = input.protect_resident(cx).unwrap();
            let successor = seed(source, 2, 8);
            let (environment, cleanup) =
                make_environment(204, config(source, 2, 32), window.text_system());
            let (reservation, candidate) =
                ready(input, cut, source, successor, &environment, &[], window);
            drop(candidate);
            drain_cleanup(&cleanup);
            let (_, other) = ready(input, cut, source, successor, &environment, &[], window);
            let current = RangePrepublicationCurrent {
                binding: successor.binding,
                history: successor.history,
                available_capacity: other.adoption_peak(),
            };
            assert_eq!(
                input.adopt_resident_successor(
                    reservation,
                    &environment,
                    other,
                    current,
                    window,
                    cx
                ),
                Err(RangePrepublicationAdoptionError::ReservationMismatch)
            );
            assert_eq!(
                input.export_restoration(original.history).unwrap(),
                original
            );
            assert!(input.resident_protection_is_current(cut));
            drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
        })
    });
    drop(input);
    window.update(|_, _| {});
    cx.run_until_parked();
    drain_cleanup(&old_cleanup);
}
