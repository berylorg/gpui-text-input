use super::*;

#[gpui::test]
fn resident_protection_refuses_select_all_left_pending_after_page_failure(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            let mut layout = input.config.layout.clone();
            layout.wrap_width += px(1.);
            input
                .set_layout(layout, input.config.style.clone(), cx)
                .unwrap();
            input.select_all(&crate::actions::SelectAll, window, cx);
            assert!(input.pending_select_all);
            let _ = input.service_admitted_geometry_for_prepaint(window, cx);
            let mut failed = false;
            while let Some(request) = input.take_request() {
                if let RangeTextInputRequest::Page(page) = request {
                    input
                        .fail_page(page.key(), crate::PageFailure::Unavailable, cx)
                        .unwrap();
                    failed = true;
                }
            }
            assert!(failed);
            input.obsolete_realization_continuation();
            assert!(input.surface().is_some());
            assert!(input.pending_select_all);
            input.set_enabled(false, cx);
            assert!(matches!(
                input.protect_resident(cx),
                Err(RangeTextInputError::NotQuiescent)
            ));
            assert!(input.resident_protection.is_none());
        })
    });
}

#[gpui::test]
fn protected_resident_rejects_stale_settlement_at_exhausted_frame_credit(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.set_enabled(false, cx);
            input.last_realization_step.remaining = 0;
            let cut = input.protect_resident(cx).unwrap();
            let before = transition_fingerprint(input);
            let key = crate::MutationKey::new(
                input.config.binding.binding(),
                input.config.binding.revision(),
                crate::OperationId::new(99_999),
            );
            let intent = crate::RangeHistoryIntent::new(
                key,
                input.config.binding,
                crate::MutationKind::Undo,
                input.history_frontier(),
                cut.seed().caret,
                cut.seed().selection,
            );
            for _ in 0..2 {
                assert!(matches!(
                    input.settle_history(intent, crate::RangeHistoryOutcome::Cancelled, window, cx),
                    Err(RangeTextInputError::Busy)
                ));
                assert!(matches!(
                    input.settle_mutation(key, crate::MutationOutcome::Cancelled, window, cx),
                    Err(RangeTextInputError::Busy)
                ));
                assert!(input.is_quiescent());
                assert!(input.resident_protection_is_current(cut));
                assert_eq!(transition_fingerprint(input), before);
            }
        })
    });
}

#[gpui::test]
fn resident_protection_requires_disabled_quiescent_publication(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    input.update(cx, |input, cx| {
        assert!(matches!(
            input.protect_resident(cx),
            Err(RangeTextInputError::Busy)
        ));
        input.set_enabled(false, cx);
        assert!(matches!(
            input.protect_resident(cx),
            Err(RangeTextInputError::NotQuiescent)
        ));
        assert!(input.resident_protection.is_none());
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        assert!(input.is_quiescent());
        let expected = input
            .export_restoration(Some(input.history_frontier()))
            .unwrap();
        let cut = input.protect_resident(cx).unwrap();
        assert_eq!(cut.seed(), expected);
        assert!(input.resident_protection_is_current(cut));
        assert!(matches!(
            input.protect_resident(cx),
            Err(RangeTextInputError::Busy)
        ));
        input.set_enabled(true, cx);
        assert!(!input.is_enabled());
        input.release_resident_protection(cut, cx).unwrap();
        assert!(!input.is_enabled());
        let successor = input.protect_resident(cx).unwrap();
        assert_ne!(cut, successor);
        assert!(matches!(
            input.release_resident_protection(cut, cx),
            Err(RangeTextInputError::Stale)
        ));
        assert!(input.resident_protection_is_current(successor));
        input.release_resident_protection(successor, cx).unwrap();
        input.set_enabled(true, cx);
        assert!(input.is_enabled());
    });
}

#[gpui::test]
fn protected_resident_rejects_work_without_retaining_intents(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.set_enabled(false, cx);
            let cut = input.protect_resident(cx).unwrap();
            let before = transition_fingerprint(input);
            assert!(matches!(
                input.request_absolute_scroll(px(10.), cx),
                Err(RangeTextInputError::Busy)
            ));
            assert!(matches!(
                input.import_restoration(cut.seed(), cx),
                Err(RangeTextInputError::Busy)
            ));
            assert!(matches!(
                input.rebind(input.config.binding, None, window, cx),
                Err(RangeTextInputError::Busy)
            ));
            assert!(matches!(
                input.platform_text_for_range(0..1, cx),
                Err(RangeTextInputError::Busy)
            ));
            assert!(matches!(
                input.lease_host_operation(),
                Err(RangeTextInputError::Busy)
            ));
            assert!(matches!(
                input.set_history_frontier(input.history_frontier(), input.history_frontier()),
                Err(RangeTextInputError::Busy)
            ));
            assert_eq!(transition_fingerprint(input), before);
            assert!(input.resident_protection_is_current(cut));
            let mut layout = input.config.layout.clone();
            layout.wrap_width += px(1.);
            assert!(matches!(
                input.set_layout(layout, input.config.style.clone(), cx),
                Err(RangeTextInputError::Busy)
            ));
            assert!(!input.resident_protection_is_current(cut));
            assert_eq!(transition_fingerprint(input), before);
            assert!(input.is_quiescent());
            input.release_resident_protection(cut, cx).unwrap();
        })
    });
}

#[gpui::test]
fn protected_resident_paints_and_loses_focus_without_source_progress(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| input.update(app, |input, _| input.focus(window)));
    cx.run_until_parked();
    let (cut, before, focus) = input.update(cx, |input, cx| {
        input.set_enabled(false, cx);
        let cut = input.protect_resident(cx).unwrap();
        (
            cut,
            transition_fingerprint(input),
            input.focus_handle.clone(),
        )
    });
    cx.update(|window, app| {
        window.blur();
        window.draw(app).clear();
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert_eq!(transition_fingerprint(input), before);
        assert_eq!(input.focus_handle, focus);
        assert!(input.focus_subscription.is_some());
        assert!(input.is_quiescent());
        assert!(input.surface().is_some());
        assert!(!input.is_surface_current_and_interactive());
        assert!(input.resident_protection_is_current(cut));
    });
    cx.simulate_resize(gpui::size(px(800.), px(80.)));
    cx.update(|window, app| window.draw(app).clear());
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        assert!(!input.resident_protection_is_current(cut));
        assert_eq!(transition_fingerprint(input), before);
        assert!(input.is_quiescent());
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _cleanup = input.dispose(window, cx);
            assert!(!input.resident_protection_is_current(cut));
            assert!(input.surface().is_none());
            assert!(matches!(
                input.release_resident_protection(cut, cx),
                Err(RangeTextInputError::NotMounted)
            ));
        })
    });
}
