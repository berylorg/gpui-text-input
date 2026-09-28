use super::*;
use gpui::Focusable;
use gpui_text_input::{PageRequest, RangeTextInputEvent, RangeTextInputRequest};

fn drive_ordinary(
    input: &gpui::Entity<RangeTextInput>,
    visual: &mut gpui::VisualTestContext,
    source: &str,
) -> Vec<PageRequest> {
    let mut requests = Vec::new();
    for id in 1..2048 {
        visual.update(|window, cx| window.draw(cx).clear());
        visual.run_until_parked();
        let request = input.update(visual, |input, _| input.take_request());
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                requests.push(request);
                visual.update(|window, cx| {
                    input.update(cx, |input, cx| {
                        input
                            .deliver_page(page_for(source, id, request), window, cx)
                            .unwrap();
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                visual.update(|window, cx| {
                    input.update(cx, |input, cx| {
                        input
                            .deliver_object_page_in_window(
                                empty_object_page(id, request),
                                window,
                                cx,
                            )
                            .unwrap();
                    })
                });
            }
            Some(
                RangeTextInputRequest::ReleasePage(_)
                | RangeTextInputRequest::ReleaseObjectPage(_)
                | RangeTextInputRequest::CancelPage(_)
                | RangeTextInputRequest::CancelObjectPage(_),
            ) => {}
            Some(other) => panic!("unexpected request: {other:?}"),
            None if input.read_with(visual, |input, _| input.is_quiescent()) => return requests,
            None => {}
        }
    }
    panic!("ordinary resident did not become quiescent");
}

#[gpui::test]
fn ordinary_resident_adoption_keeps_request_identity_focus_subscription_and_paint(
    cx: &mut TestAppContext,
) {
    let source = "alpha\nbeta\ngamma\ndelta";
    let (input, visual) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(source, 1, 32), window, cx).unwrap()
    });
    visual.simulate_resize(gpui::size(px(160.), px(64.)));
    let mut requests = drive_ordinary(&input, visual, source);
    for generation in 4..7 {
        input.update(visual, |input, cx| {
            input
                .set_presentation_generation(PresentationGeneration::new(generation), cx)
                .unwrap()
        });
        requests.extend(drive_ordinary(&input, visual, source));
    }
    let old_max = requests
        .iter()
        .map(|request| request.key().id().get())
        .max()
        .unwrap();
    let stale = requests[0];
    let focus_out = Rc::new(Cell::new(0));
    let observed = focus_out.clone();
    let subscription = visual.update(|_, cx| {
        cx.subscribe(&input, move |_, event: &RangeTextInputEvent, _| {
            if matches!(event, RangeTextInputEvent::FocusLost) {
                observed.set(observed.get() + 1);
            }
        })
    });
    let (cleanup, cut, restoration) = visual.update(|window, cx| {
        input.update(cx, |input, cx| {
            assert_eq!(input.realization_diagnostics().adopted_custody_items, 0);
            let focus = input.focus_handle(cx);
            window.focus(&focus);
            input.set_enabled(false, cx);
            let cut = input.protect_resident(cx).unwrap();
            let restoration = cut.seed();
            let mut configuration = config(source, 1, 32);
            configuration.viewport_extent = px(64.);
            configuration.presentation_generation = PresentationGeneration::new(6);
            let (environment, cleanup) = make_environment(207, configuration, window.text_system());
            let (mut session, reservation) = input
                .prepare_resident_successor(
                    cut,
                    restoration,
                    environment.clone(),
                    RangeSurfaceCharge {
                        bytes: usize::MAX,
                        items: usize::MAX,
                    },
                )
                .unwrap();
            let (candidate, _, _) = drive(&mut session, source, window.text_system(), &cleanup);
            let current = RangePrepublicationCurrent {
                binding: restoration.binding,
                history: restoration.history,
                available_capacity: candidate.adoption_peak(),
            };
            input
                .adopt_resident_successor(reservation, &environment, candidate, current, window, cx)
                .unwrap();
            assert_eq!(input.focus_handle(cx), focus);
            assert!(focus.is_focused(window));
            assert!(input.take_request().is_none());
            assert_eq!(cleanup.ownership().ready, 0);
            for id in [9001, 9002] {
                let geometry = input.surface().unwrap().geometry_key();
                let _ = input.deliver_page(page_for(source, id, stale), window, cx);
                assert_eq!(input.surface().unwrap().geometry_key(), geometry);
                assert!(input.take_request().is_none());
            }
            (cleanup, cut, restoration)
        })
    });
    visual.update(|window, cx| window.draw(cx).clear());
    visual.run_until_parked();
    input.read_with(visual, |input, _| {
        assert_eq!(
            input.export_restoration(restoration.history).unwrap(),
            restoration
        );
        assert!(!input.is_enabled());
    });
    input.update(visual, |input, cx| {
        input.release_resident_protection(cut, cx).unwrap();
        assert!(!input.is_enabled());
        input.set_enabled(true, cx);
        input
            .set_presentation_generation(PresentationGeneration::new(7), cx)
            .unwrap();
    });
    let new_requests = drive_ordinary(&input, visual, source);
    assert!(!new_requests.is_empty());
    assert!(
        new_requests
            .iter()
            .all(|request| request.key().id().get() > old_max)
    );
    let before_focus_loss = focus_out.get();
    visual.update(|window, cx| input.update(cx, |input, cx| window.focus(&input.focus_handle(cx))));
    visual.update(|window, cx| window.draw(cx).clear());
    visual.run_until_parked();
    visual.update(|window, _| window.blur());
    visual.run_until_parked();
    assert_eq!(focus_out.get(), before_focus_loss + 1);
    drain_cleanup(&cleanup);
    assert_eq!(cleanup.ownership().active, 0);
    drop(subscription);
}
