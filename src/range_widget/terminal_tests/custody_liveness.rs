use super::*;

fn stale_text_page(input: &mut RangeTextInput, id: u64) -> RangePage {
    let key = crate::PageRequestKey::adjacent(
        PageRequestId::new(id),
        binding().binding(),
        binding().revision(),
        PagePurpose::Caret,
        ByteOffset::new(0),
        PageDirection::Forward,
        SOURCE.len() as u64,
    )
    .unwrap();
    input.dispatched_pages.insert(key);
    page_for(PageRequest::new(key), id)
}

fn requested_text_page(input: &mut RangeTextInput, id: u64) -> RangePage {
    let PageDemand::Requested(request) = input
        .residency
        .demand(
            PageRequestId::new(id),
            PagePurpose::Caret,
            PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(0),
                direction: PageDirection::Forward,
                max_payload_bytes: SOURCE.len() as u64,
            },
        )
        .unwrap()
    else {
        panic!("fresh response must own a pending residency request")
    };
    input.dispatched_pages.insert(request.key());
    page_for(request, id)
}

fn stale_object_page(input: &mut RangeTextInput, id: u64) -> ObjectPage {
    let key = crate::ObjectRequestKey::new(
        ObjectRequestId::new(id),
        binding().binding(),
        binding().revision(),
        input.config.presentation_generation,
        ObjectPurpose::Restoration,
        ObjectDemandEnvelope::anchor(ByteOffset::new(0), None, ObjectDirection::Forward, 1, 4_096)
            .unwrap(),
    )
    .unwrap();
    input.dispatched_object_pages.insert(key);
    ObjectPage::new(
        ObjectPageId::new(id),
        key,
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap()
}

fn drain_release_requests(input: &mut RangeTextInput) {
    while let Some(request) = input.take_request() {
        assert!(matches!(
            request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_)
        ));
    }
}

#[gpui::test]
fn scheduled_frames_drain_mixed_text_object_and_alias_custody_tail(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            input.spend_realization_credit();
            let text = requested_text_page(input, 610_000);
            let object = stale_object_page(input, 610_001);
            let alias = stale_text_page(input, 610_002);
            input.dispatched_pages.remove(&alias.key());
            input
                .admit_response_custody(super::super::response_custody::RangeResponseCustody::Page(
                    text,
                ))
                .unwrap();
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::Object(object),
                )
                .unwrap();
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::AliasFanout(
                        super::super::response_custody::AliasFanout {
                            page: alias,
                            cursor: 0,
                            matched: true,
                        },
                    ),
                )
                .unwrap();
            assert!(!input.service_response_custody(window, cx).unwrap());
            assert_eq!(input.response_custody.len(), 3);
            assert!(input.realization_continuation_scheduled);
            let frame_before = input.realization_frame_generation;
            let mut continuation_frames = 0;
            while !input.response_custody.is_empty() {
                assert!(continuation_frames < 8, "custody tail did not quiesce");
                assert!(
                    input.realization_continuation_scheduled,
                    "frame {continuation_frames}, custody {:?}",
                    input.response_custody
                );
                input.begin_realization_frame();
                let _ = input.service_response_custody(window, cx);
                assert!(input.realization_diagnostics().frame.spent <= 1);
                continuation_frames += 1;
            }
            assert_eq!(continuation_frames, 3);
            assert_eq!(
                input.realization_frame_generation,
                frame_before.wrapping_add(3)
            );
            assert!(!input.realization_continuation_scheduled);
            assert_eq!(input.dispatched_pages.len(), 0);
            assert_eq!(input.dispatched_object_pages.len(), 0);
            drain_release_requests(input);
            assert!(input.is_quiescent());
        })
    });
}

#[gpui::test]
fn retained_failure_rotates_behind_tail_without_spinning_when_alone(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let retry = stale_text_page(input, 610_050);
            let retry_key = retry.key();
            let tail = stale_object_page(input, 610_051);
            input
                .admit_response_custody(super::super::response_custody::RangeResponseCustody::Page(
                    retry,
                ))
                .unwrap();
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::Object(tail),
                )
                .unwrap();

            input.begin_realization_frame();
            assert!(matches!(
                input.service_response_custody(window, cx),
                Err(RangeTextInputError::Stale)
            ));
            assert!(matches!(
                input.response_custody.front(),
                Some(super::super::response_custody::RangeResponseCustody::Object(_))
            ));
            assert!(input.realization_continuation_scheduled);

            input.begin_realization_frame();
            assert!(matches!(
                input.service_response_custody(window, cx),
                Err(RangeTextInputError::Stale)
            ));
            assert_eq!(input.response_custody.len(), 1);
            assert!(input.realization_continuation_scheduled);

            input.begin_realization_frame();
            assert!(matches!(
                input.service_response_custody(window, cx),
                Err(RangeTextInputError::Stale)
            ));
            assert_eq!(input.response_custody.len(), 1);
            assert!(!input.realization_continuation_scheduled);

            let PageDemand::Requested(request) = input
                .residency
                .demand(retry_key.id(), retry_key.purpose(), retry_key.demand())
                .unwrap()
            else {
                panic!("external state change must make the retained response admissible")
            };
            assert_eq!(request.key(), retry_key);
            input.schedule_realization_continuation(cx);
            input.begin_realization_frame();
            assert!(input.service_response_custody(window, cx).unwrap());
            assert!(input.response_custody.is_empty());
            assert_eq!(input.dispatched_pages.len(), 0);
            assert_eq!(input.dispatched_object_pages.len(), 0);
            drain_release_requests(input);
        })
    });
}

#[gpui::test]
fn public_text_arrival_wakes_a_sleeping_retained_text_head(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            let sleeping = stale_text_page(input, 610_070);
            let sleeping_key = sleeping.key();
            input.deliver_page(sleeping, window, cx).unwrap();
            assert_eq!(input.response_custody.len(), 1);
            assert!(!input.realization_continuation_scheduled);

            let arrival = requested_text_page(input, 610_071);
            let arrival_key = arrival.key();
            input.deliver_page(arrival, window, cx).unwrap();
            assert_eq!(input.response_custody.len(), 2);
            assert!(matches!(
                input.response_custody.front(),
                Some(super::super::response_custody::RangeResponseCustody::Page(page))
                    if page.key() == arrival_key
            ));
            assert!(matches!(
                input.response_custody.back(),
                Some(super::super::response_custody::RangeResponseCustody::Page(page))
                    if page.key() == sleeping_key
            ));
            assert!(input.realization_continuation_scheduled);

            let _ = input.dispose(window, cx);
            assert!(input.response_custody.is_empty());
            assert!(!input.realization_continuation_scheduled);
        })
    });
    cx.run_until_parked();
}

#[gpui::test]
fn public_windowless_object_arrival_wakes_a_non_object_head(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let sleeping_key = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            let sleeping = stale_text_page(input, 610_080);
            let key = sleeping.key();
            input.deliver_page(sleeping, window, cx).unwrap();
            assert!(!input.realization_continuation_scheduled);
            key
        })
    });
    input.update(cx, |input, cx| {
        let object = stale_object_page(input, 610_081);
        let object_key = object.key();
        input.deliver_object_page(object, cx).unwrap();
        assert_eq!(input.response_custody.len(), 2);
        assert!(matches!(
            input.response_custody.front(),
            Some(super::super::response_custody::RangeResponseCustody::Page(page))
                if page.key() == sleeping_key
        ));
        assert!(matches!(
            input.response_custody.back(),
            Some(super::super::response_custody::RangeResponseCustody::Object(page))
                if page.key() == object_key
        ));
        assert!(input.realization_continuation_scheduled);
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
            assert!(input.response_custody.is_empty());
            assert!(!input.realization_continuation_scheduled);
        })
    });
    cx.run_until_parked();
}

#[gpui::test]
fn public_windowed_object_arrival_wakes_a_sleeping_text_head(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            let sleeping = stale_text_page(input, 610_090);
            let sleeping_key = sleeping.key();
            input.deliver_page(sleeping, window, cx).unwrap();
            assert_eq!(input.response_custody.len(), 1);
            assert!(!input.realization_continuation_scheduled);

            let arrival = stale_object_page(input, 610_091);
            let arrival_key = arrival.key();
            assert!(matches!(
                input.deliver_object_page_in_window(arrival, window, cx),
                Ok(()) | Err(RangeTextInputError::Stale)
            ));
            assert_eq!(input.response_custody.len(), 2);
            assert!(matches!(
                input.response_custody.front(),
                Some(super::super::response_custody::RangeResponseCustody::Object(page))
                    if page.key() == arrival_key
            ));
            assert!(matches!(
                input.response_custody.back(),
                Some(super::super::response_custody::RangeResponseCustody::Page(page))
                    if page.key() == sleeping_key
            ));
            assert!(input.realization_continuation_scheduled);

            let _ = input.dispose(window, cx);
            assert!(input.response_custody.is_empty());
            assert!(!input.realization_continuation_scheduled);
        })
    });
    cx.run_until_parked();
}

#[gpui::test]
fn windowless_object_custody_schedules_each_terminal_tail_unit(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let first = stale_object_page(input, 610_100);
        let second = stale_object_page(input, 610_101);
        input
            .admit_response_custody(
                super::super::response_custody::RangeResponseCustody::Object(first),
            )
            .unwrap();
        input
            .admit_response_custody(
                super::super::response_custody::RangeResponseCustody::Object(second),
            )
            .unwrap();
        input.begin_realization_frame();
        assert!(matches!(
            input.service_object_response_custody(cx),
            Err(RangeTextInputError::Stale)
        ));
        assert_eq!(input.response_custody.len(), 1);
        assert!(input.realization_continuation_scheduled);

        input.begin_realization_frame();
        assert!(matches!(
            input.service_object_response_custody(cx),
            Err(RangeTextInputError::Stale)
        ));
        assert!(input.response_custody.is_empty());
        assert!(!input.realization_continuation_scheduled);
        assert_eq!(input.dispatched_object_pages.len(), 0);
        drain_release_requests(input);
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn disposal_obsoletes_a_scheduled_mixed_custody_tail(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.limits.max_realization_work_per_frame = 1;
        RangeTextInput::new(configuration, window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let alias = stale_text_page(input, 610_200);
            let object = stale_object_page(input, 610_201);
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::AliasFanout(
                        super::super::response_custody::AliasFanout {
                            page: alias,
                            cursor: 0,
                            matched: false,
                        },
                    ),
                )
                .unwrap();
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::Object(object),
                )
                .unwrap();
            input.begin_realization_frame();
            assert!(input.service_response_custody(window, cx).unwrap());
            assert_eq!(input.response_custody.len(), 2);
            assert!(input.realization_continuation_scheduled);
            let _ = input.dispose(window, cx);
            assert!(input.response_custody.is_empty());
            assert!(!input.realization_continuation_scheduled);
        })
    });
    cx.run_until_parked();
    input.read_with(cx, |input, _| {
        let current = input.realization_diagnostics().current;
        assert_eq!(current.response_custody_count, 0);
        assert_eq!(current.response_custody_bytes, 0);
        assert_eq!(current.response_custody_items, 0);
        assert_eq!(current.request_storage_bytes, 0);
        assert!(!input.realization_continuation_scheduled);
    });
}
