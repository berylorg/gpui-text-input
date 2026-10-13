use super::*;
use gpui::Focusable;
use gpui_text_input::{PageFailure, RangeTextInputError, RangeTextInputRequest};

fn combined(
    input: &RangeTextInput,
    environment: &RangePrepublicationEnvironment,
) -> RangeSurfaceCharge {
    let old = input.realization_diagnostics().current;
    RangeSurfaceCharge {
        bytes: old.owned_bytes + environment.config().limits.max_surface_bytes,
        items: old.owned_items + environment.config().limits.max_surface_items,
    }
}

#[gpui::test]
fn ordinary_adoption_rejects_source_replacement_and_keeps_the_prior_cut(cx: &mut TestAppContext) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let target = "different text";
    let original = seed(source, 1, 8);
    let successor = seed(target, 2, 1);
    let window = cx.add_empty_window();
    let (input, old_cleanup) =
        window.update(|window, cx| resident_adoption::mounted(source, original, &[], window, cx));
    window.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            let prior = input
                .protect_published_resident(original.history.unwrap(), cx)
                .unwrap();
            let (environment, cleanup) =
                make_environment(309, config(target, 2, 32), window.text_system());
            let (mut session, reservation) = input
                .prepare_source_replacement_successor(
                    prior,
                    successor,
                    environment.clone(),
                    combined(input, &environment),
                )
                .unwrap();
            let (candidate, _, _) = drive(&mut session, target, window.text_system(), &cleanup);
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
                Err(RangePrepublicationAdoptionError::ReservationMismatch)
            );
            assert_eq!(
                input.export_restoration(original.history).unwrap(),
                original
            );
            assert!(input.resident_protection_is_current(prior));
            input.release_resident_protection(prior, cx).unwrap();
            assert!(input.dispose(window, cx).is_empty());
            drop(session);
            drain_cleanup(&cleanup);
            drain_cleanup(&old_cleanup);
            assert_eq!(cleanup.ownership().active, 0);
            assert_eq!(old_cleanup.ownership().active, 0);
        })
    });
}

#[gpui::test]
fn source_replacement_adopts_changed_positions_and_renews_the_same_resident(
    cx: &mut TestAppContext,
) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let target = "new text\nsecond line";
    let original = seed(source, 1, 8);
    let mut successor = seed(target, 2, 2);
    successor.selection.anchor = position(7);
    successor.scroll.position = position(4);
    let window = cx.add_empty_window();
    let (input, old_cleanup) =
        window.update(|window, cx| resident_adoption::mounted(source, original, &[], window, cx));
    window.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            let focus = input.focus_handle(cx);
            let prior = input
                .protect_published_resident(original.history.unwrap(), cx)
                .unwrap();
            let (environment, cleanup) =
                make_environment(301, config(target, 2, 32), window.text_system());
            let capacity = combined(input, &environment);
            assert!(matches!(
                input.prepare_resident_successor(prior, successor, environment.clone(), capacity),
                Err(RangePrepublicationFailure::SourceMismatch)
            ));
            assert_eq!(cleanup.ownership().active, 0);
            let (mut session, reservation) = input
                .prepare_source_replacement_successor(
                    prior,
                    successor,
                    environment.clone(),
                    capacity,
                )
                .unwrap();
            let (candidate, _, _) = drive(&mut session, target, window.text_system(), &cleanup);
            let current = RangePrepublicationCurrent {
                binding: successor.binding,
                history: successor.history,
                available_capacity: candidate.adoption_peak(),
            };
            let next = input
                .adopt_protected_resident_successor(
                    reservation,
                    &environment,
                    candidate,
                    current,
                    window,
                    cx,
                )
                .unwrap();
            assert_eq!(next.seed(), successor);
            assert_eq!(
                input.export_restoration(successor.history).unwrap(),
                successor
            );
            assert_eq!(input.focus_handle(cx), focus);
            assert_eq!(input.surface().unwrap().binding(), successor.binding);
            assert!(input.resident_protection_is_current(next));
            assert!(!input.resident_protection_is_current(prior));
            assert!(matches!(
                input.release_resident_protection(prior, cx),
                Err(RangeTextInputError::Stale)
            ));
            assert!(!input.is_enabled());
            assert!(input.take_request().is_none());
            input.release_resident_protection(next, cx).unwrap();
            assert!(input.dispose(window, cx).is_empty());
            drop(session);
            drain_cleanup(&cleanup);
            drain_cleanup(&old_cleanup);
            assert_eq!(cleanup.ownership().active, 0);
            assert_eq!(old_cleanup.ownership().active, 0);
        })
    });
}

#[gpui::test]
fn source_replacement_rejects_another_sessions_candidate_without_touching_predecessor(
    cx: &mut TestAppContext,
) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let target = "different text";
    let original = seed(source, 1, 8);
    let successor = seed(target, 2, 1);
    let window = cx.add_empty_window();
    let (input, old_cleanup) =
        window.update(|window, cx| resident_adoption::mounted(source, original, &[], window, cx));
    window.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            let prior = input
                .protect_published_resident(original.history.unwrap(), cx)
                .unwrap();
            let before = input.realization_diagnostics().current;
            let (environment, cleanup) =
                make_environment(302, config(target, 2, 32), window.text_system());
            let capacity = combined(input, &environment);
            let (session, reservation) = input
                .prepare_source_replacement_successor(
                    prior,
                    successor,
                    environment.clone(),
                    capacity,
                )
                .unwrap();
            drop(session);
            let mut other =
                RangePrepublicationSession::new(successor, environment.clone()).unwrap();
            let (candidate, _, _) = drive(&mut other, target, window.text_system(), &cleanup);
            let current = RangePrepublicationCurrent {
                binding: successor.binding,
                history: successor.history,
                available_capacity: candidate.adoption_peak(),
            };
            assert!(matches!(
                input.adopt_protected_resident_successor(
                    reservation,
                    &environment,
                    candidate,
                    current,
                    window,
                    cx
                ),
                Err(RangePrepublicationAdoptionError::ReservationMismatch)
            ));
            assert_eq!(input.realization_diagnostics().current, before);
            assert_eq!(
                input.export_restoration(original.history).unwrap(),
                original
            );
            assert!(input.resident_protection_is_current(prior));
            drop(other);
            drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
            input.release_resident_protection(prior, cx).unwrap();
            assert!(input.dispose(window, cx).is_empty());
            drain_cleanup(&old_cleanup);
        })
    });
}

#[gpui::test]
fn published_capture_retains_actual_old_surface_after_failed_logical_rebind(
    cx: &mut TestAppContext,
) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let target = "different text";
    let original = seed(source, 1, 8);
    let successor = seed(target, 2, 1);
    let window = cx.add_empty_window();
    let (input, old_cleanup) =
        window.update(|window, cx| resident_adoption::mounted(source, original, &[], window, cx));
    window.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.set_enabled(false, cx);
            assert!(matches!(
                input.protect_published_resident(successor.history.unwrap(), cx),
                Err(RangeTextInputError::Stale)
            ));
            input
                .rebind(successor.binding, Some(successor.selection), window, cx)
                .unwrap();
            assert!(matches!(
                input.protect_published_resident(original.history.unwrap(), cx),
                Err(RangeTextInputError::NotQuiescent)
            ));
            let mut failed = false;
            for _ in 0..128 {
                let Some(request) = input.take_request() else {
                    break;
                };
                match request {
                    RangeTextInputRequest::Page(request) => {
                        input
                            .fail_page(request.key(), PageFailure::Unavailable, cx)
                            .unwrap();
                        failed = true;
                    }
                    RangeTextInputRequest::ReleasePage(_)
                    | RangeTextInputRequest::ReleaseObjectPage(_)
                    | RangeTextInputRequest::CancelPage(_)
                    | RangeTextInputRequest::CancelObjectPage(_) => {}
                    other => panic!("unexpected effect: {other:?}"),
                }
            }
            assert!(failed);
            assert!(input.is_quiescent());
            assert_eq!(input.surface().unwrap().binding(), original.binding);
            assert!(input.protect_resident(cx).is_err());
            let prior = input
                .protect_published_resident(original.history.unwrap(), cx)
                .unwrap();
            assert_eq!(prior.seed(), original);
            assert!(input.resident_protection_is_current(prior));
            let (environment, cleanup) =
                make_environment(303, config(target, 2, 32), window.text_system());
            let (mut session, reservation) = input
                .prepare_source_replacement_successor(
                    prior,
                    successor,
                    environment.clone(),
                    combined(input, &environment),
                )
                .unwrap();
            let (candidate, _, _) = drive(&mut session, target, window.text_system(), &cleanup);
            let current = RangePrepublicationCurrent {
                binding: successor.binding,
                history: successor.history,
                available_capacity: candidate.adoption_peak(),
            };
            let next = input
                .adopt_protected_resident_successor(
                    reservation,
                    &environment,
                    candidate,
                    current,
                    window,
                    cx,
                )
                .unwrap();
            assert_eq!(next.seed(), successor);
            input.release_resident_protection(next, cx).unwrap();
            assert!(input.dispose(window, cx).is_empty());
            drop(session);
            drain_cleanup(&cleanup);
            drain_cleanup(&old_cleanup);
            assert_eq!(cleanup.ownership().active, 0);
            assert_eq!(old_cleanup.ownership().active, 0);
        })
    });
}

#[gpui::test]
fn source_replacement_capacity_and_environment_refusals_preserve_protection(
    cx: &mut TestAppContext,
) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let target = "different text";
    let original = seed(source, 1, 8);
    let successor = seed(target, 2, 1);
    let window = cx.add_empty_window();
    let (input, old_cleanup) =
        window.update(|window, cx| resident_adoption::mounted(source, original, &[], window, cx));
    window.update(|window, cx| {
        input.update(cx, |input, cx| {
            assert!(matches!(
                input.protect_published_resident(original.history.unwrap(), cx),
                Err(RangeTextInputError::Busy)
            ));
            input.set_enabled(false, cx);
            let prior = input
                .protect_published_resident(original.history.unwrap(), cx)
                .unwrap();
            let before = input.realization_diagnostics().current;
            let (environment, cleanup) =
                make_environment(304, config(target, 2, 32), window.text_system());
            assert!(matches!(
                input.prepare_source_replacement_successor(
                    prior,
                    successor,
                    environment.clone(),
                    RangeSurfaceCharge {
                        bytes: before.owned_bytes - 1,
                        items: before.owned_items
                    }
                ),
                Err(RangePrepublicationFailure::InitialCapacityDenied)
            ));
            assert_eq!(cleanup.ownership().active, 0);
            let (mut cancelled, cancelled_reservation) = input
                .prepare_source_replacement_successor(
                    prior,
                    successor,
                    environment.clone(),
                    combined(input, &environment),
                )
                .unwrap();
            assert!(!cancelled.service(window.text_system()).effects.is_empty());
            cancelled.cancel();
            drop(cancelled);
            drop(cancelled_reservation);
            drain_cleanup(&cleanup);
            assert_eq!(cleanup.ownership().active, 0);
            assert!(input.resident_protection_is_current(prior));
            assert_eq!(input.realization_diagnostics().current, before);
            let (mut session, reservation) = input
                .prepare_source_replacement_successor(
                    prior,
                    successor,
                    environment.clone(),
                    combined(input, &environment),
                )
                .unwrap();
            let (candidate, _, _) = drive(&mut session, target, window.text_system(), &cleanup);
            let current = RangePrepublicationCurrent {
                binding: successor.binding,
                history: successor.history,
                available_capacity: candidate.adoption_peak(),
            };
            let (wrong_environment, wrong_cleanup) =
                make_environment(305, config(target, 2, 32), window.text_system());
            assert!(matches!(
                input.adopt_protected_resident_successor(
                    reservation,
                    &wrong_environment,
                    candidate,
                    current,
                    window,
                    cx
                ),
                Err(RangePrepublicationAdoptionError::EnvironmentMismatch)
            ));
            assert!(input.resident_protection_is_current(prior));
            assert_eq!(input.realization_diagnostics().current, before);
            assert_eq!(
                input.export_restoration(original.history).unwrap(),
                original
            );
            drop(session);
            drain_cleanup(&cleanup);
            drain_cleanup(&wrong_cleanup);
            assert_eq!(cleanup.ownership().active, 0);
            input.release_resident_protection(prior, cx).unwrap();
            assert!(input.dispose(window, cx).is_empty());
            drain_cleanup(&old_cleanup);
        })
    });
}
