use super::*;
use gpui::Focusable;
use gpui_text_input::RangeTextInputError;

#[gpui::test]
fn resident_protection_preserves_non_origin_directed_selection_and_history(
    cx: &mut TestAppContext,
) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let mut restoration = seed(source, 5, 8);
    restoration.selection.anchor = position(10);
    let window = cx.add_empty_window();
    let (input, cleanup) = window.update(|window, cx| {
        let (environment, cleanup) =
            make_environment(94, config(source, 5, 32), window.text_system());
        let mut session =
            RangePrepublicationSession::new(restoration, environment.clone()).unwrap();
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
    });
    window.update(|_, cx| {
        input.update(cx, |input, cx| {
            let focus = input.focus_handle(cx);
            input.set_enabled(false, cx);
            let before = input.realization_diagnostics().current;
            let cut = input.protect_resident(cx).unwrap();
            assert_eq!(cut.seed(), restoration);
            assert!(matches!(
                input.request_absolute_scroll(px(20.), cx),
                Err(RangeTextInputError::Busy)
            ));
            assert_eq!(
                input.export_restoration(restoration.history).unwrap(),
                restoration
            );
            assert_eq!(input.history_frontier(), restoration.history.unwrap());
            assert_eq!(input.focus_handle(cx), focus);
            assert_eq!(input.realization_diagnostics().current, before);
            assert!(input.take_request().is_none());
            input.release_resident_protection(cut, cx).unwrap();
            assert!(!input.is_enabled());
        })
    });
    drop(input);
    window.update(|_, _| {});
    cx.run_until_parked();
    drain_cleanup(&cleanup);
    assert_eq!(cleanup.ownership().active, 0);
}
