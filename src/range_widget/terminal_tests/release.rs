use super::*;

#[gpui::test]
fn huge_viewport_filler_reanchor_advances_independently_of_clamped_scroll(
    cx: &mut gpui::TestAppContext,
) {
    let source = (0..64)
        .map(|line| format!("anchor-{line:03}\n"))
        .collect::<String>();
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(182_001),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 64),
    );
    configuration.viewport_extent = px(4_096.);
    configuration.limits.max_realized_block_extent = px(16.);
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    cx.simulate_resize(gpui::size(px(800.), px(4_096.)));
    drive_surface_for_source(&input, cx, &source);

    let mut previous_anchor = Pixels::ZERO;
    for _ in 0..80 {
        let next = input.read_with(cx, |input, _| {
            let surface = input.surface().unwrap();
            assert_eq!(surface.scroll_block(), Pixels::ZERO);
            let anchor = input.desired.realization_anchor_block;
            let realized_end = (anchor + px(16.)).min(surface.content_height());
            let fillers = surface.fillers().collect::<Vec<_>>();
            if anchor > Pixels::ZERO {
                assert!(fillers.iter().any(|filler| {
                    filler.block_start() == Pixels::ZERO && filler.block_end() == anchor
                }));
            }
            if realized_end < surface.content_height() {
                assert!(fillers.iter().any(|filler| {
                    filler.block_start() == realized_end
                        && filler.block_end() == surface.content_height()
                }));
            }
            fillers
                .into_iter()
                .find(|filler| filler.successor_block() > anchor)
                .map(|filler| filler.successor_block())
        });
        let Some(next) = next else {
            break;
        };
        assert!(next > previous_anchor);
        input.update(cx, |input, cx| {
            input.request_filler_reanchor(next, cx).unwrap();
        });
        drive_surface_for_source(&input, cx, &source);
        previous_anchor = next;
    }
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.scroll_block(), Pixels::ZERO);
        assert_eq!(surface.filler_count(), 1);
        let leading = surface.fillers().next().unwrap();
        assert_eq!(leading.block_start(), Pixels::ZERO);
        assert_eq!(leading.block_end() + px(16.), surface.content_height());
        assert!(
            surface
                .hit_test_composite(gpui::point(px(4.), leading.block_end() - px(4.)))
                .is_none()
        );
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn occupied_response_custody_returns_geometry_payloads_without_consuming_reservations(
    cx: &mut gpui::TestAppContext,
) {
    let (text_input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let text_request = match text_input
        .update(cx, |input, _| input.take_request())
        .unwrap()
    {
        RangeTextInputRequest::Page(request) => request,
        other => panic!("geometry text request: {other:?}"),
    };
    let text_page = page_for(text_request, 182_700);
    text_input.update(cx, |input, _| {
        let filler = text_page.clone();
        while input.response_custody.len() < input.response_custody.capacity() {
            input
                .admit_response_custody(super::super::response_custody::RangeResponseCustody::Page(
                    filler.clone(),
                ))
                .unwrap();
        }
    });
    let text_page = match cx
        .update(|window, app| {
            text_input.update(app, |input, cx| input.deliver_page(text_page, window, cx))
        })
        .unwrap_err()
    {
        RangeTextInputError::PageResponseCapacity(page) => page,
        other => panic!("occupied text custody: {other:?}"),
    };
    text_input.update(cx, |input, _| {
        assert!(input.dispatched_pages.contains(&text_page.key()));
        input.response_custody.clear();
    });
    cx.update(|window, app| {
        text_input.update(app, |input, cx| {
            input.begin_realization_frame();
            input.deliver_page(text_page, window, cx).unwrap();
        })
    });

    let source = "line\n".repeat(24);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(182_701),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 24),
    );
    let (object_input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let key = stage_terminal_target_object_response(&object_input, cx, &source);
    let object_page = empty_terminal_object_response(key);
    object_input.update(cx, |input, _| {
        let filler = object_page.clone();
        while input.response_custody.len() < input.response_custody.capacity() {
            input
                .admit_response_custody(
                    super::super::response_custody::RangeResponseCustody::Object(filler.clone()),
                )
                .unwrap();
        }
    });
    let object_page = match cx
        .update(|window, app| {
            object_input.update(app, |input, cx| {
                input.deliver_object_page_in_window(object_page, window, cx)
            })
        })
        .unwrap_err()
    {
        RangeTextInputError::ObjectResponseCapacity(page) => page,
        other => panic!("occupied object custody: {other:?}"),
    };
    object_input.update(cx, |input, _| {
        assert!(input.dispatched_object_pages.contains(&object_page.key()));
        input.response_custody.clear();
    });
    cx.update(|window, app| {
        object_input.update(app, |input, cx| {
            input.begin_realization_frame();
            input
                .deliver_object_page_in_window(object_page, window, cx)
                .unwrap();
        })
    });
}

#[gpui::test]
fn end_clamped_scroll_keeps_position_while_filler_anchor_advances(cx: &mut gpui::TestAppContext) {
    let source = (0..32)
        .map(|line| format!("end-{line:03}\n"))
        .collect::<String>();
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(182_002),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 32),
    );
    configuration.viewport_extent = px(80.);
    configuration.limits.max_realized_block_extent = px(16.);
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    cx.simulate_resize(gpui::size(px(800.), px(80.)));
    drive_surface_for_source(&input, cx, &source);
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(100_000.), cx).unwrap();
    });
    drive_surface_for_source(&input, cx, &source);
    let clamped = input.read_with(cx, |input, _| input.surface().unwrap().scroll_block());

    for _ in 0..8 {
        let filler = input.read_with(cx, |input, _| {
            let anchor = input.desired.realization_anchor_block;
            input
                .surface()
                .unwrap()
                .fillers()
                .find(|filler| filler.successor_block() > anchor)
        });
        let Some(filler) = filler else {
            break;
        };
        input.update(cx, |input, cx| {
            input
                .request_filler_reanchor(filler.block_start() - clamped, cx)
                .unwrap();
        });
        drive_surface_for_source(&input, cx, &source);
        input.read_with(cx, |input, _| {
            assert_eq!(input.surface().unwrap().scroll_block(), clamped);
        });
    }
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.filler_count(), 1);
        let leading = surface.fillers().next().unwrap();
        assert_eq!(leading.block_start(), clamped);
        assert_eq!(leading.block_end() + px(16.), surface.content_height());
    });
}

#[gpui::test]
fn response_custody_exact_fit_multiple_slots_and_disposal_release(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let make_page = |id, purpose| {
        let key = crate::PageRequestKey::adjacent(
            PageRequestId::new(id),
            binding().binding(),
            binding().revision(),
            purpose,
            ByteOffset::new(0),
            PageDirection::Forward,
            SOURCE.len() as u64,
        )
        .unwrap();
        page_for(PageRequest::new(key), id)
    };
    let first = make_page(182_100, PagePurpose::Caret);
    let second = make_page(182_101, PagePurpose::Viewport);
    input.update(cx, |input, _| {
        input.dispatched_pages.insert(first.key());
        input.dispatched_pages.insert(second.key());
        let current = input.current_realization_ownership();
        let charge = first.retained_charge();
        let exact = RangeSurfaceCharge {
            bytes: current.owned_bytes + charge.bytes() - std::mem::size_of::<RangePage>()
                + charge.bytes(),
            items: current.owned_items + charge.items() - 1 + charge.items(),
        };
        input.config.limits.max_surface_bytes = exact.bytes - 1;
        assert!(matches!(
            input.admit_response_custody(
                super::super::response_custody::RangeResponseCustody::Page(first.clone())
            ),
            Err(super::super::response_custody::RangeResponseCustody::Page(
                _
            ))
        ));
        assert_eq!(input.response_custody.len(), 0);
        input.config.limits.max_surface_bytes = exact.bytes;
        input
            .admit_response_custody(super::super::response_custody::RangeResponseCustody::Page(
                first,
            ))
            .unwrap();
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
        input
            .admit_response_custody(super::super::response_custody::RangeResponseCustody::Page(
                second,
            ))
            .unwrap();
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.response_custody_count, 2);
        assert_eq!(diagnostics.high_water.response_custody_count, 2);
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
            let current = input.realization_diagnostics().current;
            assert_eq!(current.response_custody_count, 0);
            assert_eq!(current.response_custody_bytes, 0);
            assert_eq!(current.response_custody_items, 0);
            assert_eq!(current.request_storage_bytes, 0);
            assert_eq!(current.request_storage_items, 0);
            assert_eq!(current.dispatched_record_bytes, 0);
            assert_eq!(current.dispatched_record_items, 0);
            assert_eq!(current.page_alias_storage_bytes, 0);
            assert_eq!(current.page_alias_storage_items, 0);
            assert_eq!(current.pending_configuration_bytes, 0);
            assert_eq!(current.pending_configuration_items, 0);
        })
    });
}

#[gpui::test]
fn exhausted_direct_rebind_is_retained_and_services_on_next_frame(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    let prior = input.read_with(cx, |input, _| format!("{:?}", input.surface()));
    let successor = RangeBinding::new(
        binding().binding(),
        SourceRevision::new(2),
        binding().extent(),
    );
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_realization_work_per_frame = 1;
            input.begin_realization_frame();
            input.spend_realization_credit();
            assert!(matches!(
                input.rebind(successor, None, window, cx),
                Err(RangeTextInputError::Busy)
            ));
            assert_eq!(
                input
                    .realization_diagnostics()
                    .current
                    .pending_rebind_intents,
                1
            );
            assert_eq!(format!("{:?}", input.surface()), prior);
        })
    });
    cx.update(|window, app| window.draw(app).clear());
    drive_surface_for_source(&input, cx, SOURCE);
    input.read_with(cx, |input, _| {
        assert_eq!(input.config.binding, successor);
        assert_eq!(input.surface().unwrap().binding(), successor);
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .pending_rebind_intents,
            0
        );
    });
}

#[gpui::test]
fn exhausted_presentation_and_rebind_compose_in_both_orders(cx: &mut gpui::TestAppContext) {
    for rebind_first in [false, true] {
        let (input, cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
        drive_initial_surface(&input, cx);
        let successor = RangeBinding::new(
            binding().binding(),
            SourceRevision::new(2),
            binding().extent(),
        );
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.config.limits.max_realization_work_per_frame = 1;
                input.begin_realization_frame();
                input.spend_realization_credit();
                if rebind_first {
                    assert!(matches!(
                        input.rebind(successor, None, window, cx),
                        Err(RangeTextInputError::Busy)
                    ));
                }
                input
                    .set_presentation_generation(PresentationGeneration::new(2), cx)
                    .unwrap();
                if !rebind_first {
                    assert!(matches!(
                        input.rebind(successor, None, window, cx),
                        Err(RangeTextInputError::Busy)
                    ));
                }
                assert_eq!(
                    input.pending_presentation_intent,
                    Some(PresentationGeneration::new(2))
                );
                assert_eq!(
                    input
                        .realization_diagnostics()
                        .current
                        .pending_rebind_intents,
                    1
                );
            })
        });
        cx.update(|window, app| window.draw(app).clear());
        drive_surface_for_source(&input, cx, SOURCE);
        input.read_with(cx, |input, _| {
            let surface = input.surface().unwrap();
            assert_eq!(input.config.binding, successor);
            assert_eq!(surface.binding(), successor);
            assert_eq!(
                input.config.presentation_generation,
                PresentationGeneration::new(2)
            );
            assert!(input.pending_presentation_intent.is_none());
            assert!(input.pending_rebind_intent.is_none());
            assert!(input.is_quiescent());
        });
    }
}

#[gpui::test]
fn public_response_capacity_returns_exact_text_custody_for_retry(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let (page, exact) = input.update(cx, |input, _| {
        let PageDemand::Requested(request) = input
            .residency
            .demand(
                PageRequestId::new(182_200),
                PagePurpose::Caret,
                PageDemandEnvelope::Adjacent {
                    anchor: ByteOffset::new(0),
                    direction: PageDirection::Forward,
                    max_payload_bytes: SOURCE.len() as u64,
                },
            )
            .unwrap()
        else {
            panic!("caret request")
        };
        input.dispatched_pages.insert(request.key());
        let page = page_for(request, 182_200);
        let current = input.current_realization_ownership();
        let charge = page.retained_charge();
        let exact = RangeSurfaceCharge {
            bytes: current.owned_bytes + charge.bytes() - std::mem::size_of::<RangePage>()
                + charge.bytes(),
            items: current.owned_items + charge.items() - 1 + charge.items(),
        };
        (page, exact)
    });
    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = exact.bytes - 1;
        input.config.limits.max_surface_items = exact.items;
    });
    let rejected = cx
        .update(|window, app| input.update(app, |input, cx| input.deliver_page(page, window, cx)));
    let RangeTextInputError::PageResponseCapacity(page) = rejected.unwrap_err() else {
        panic!("typed page custody")
    };
    input.read_with(cx, |input, _| {
        assert_eq!(input.response_custody.len(), 0);
        assert!(input.dispatched_pages.contains(&page.key()));
    });
    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = exact.bytes;
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap();
            assert_eq!(input.response_custody.len(), 0);
        })
    });
}

#[gpui::test]
fn public_response_capacity_returns_exact_object_custody_for_retry(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let page = input.update(cx, |input, _| {
        let demand = ObjectDemandEnvelope::anchor(
            ByteOffset::new(0),
            None,
            ObjectDirection::Forward,
            1,
            4_096,
        )
        .unwrap();
        let key = crate::ObjectRequestKey::new(
            ObjectRequestId::new(182_300),
            binding().binding(),
            binding().revision(),
            input.config.presentation_generation,
            ObjectPurpose::Viewport,
            demand,
        )
        .unwrap();
        input.dispatched_object_pages.insert(key);
        ObjectPage::new(
            ObjectPageId::new(182_300),
            key,
            vec![],
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::EnvelopeBoundary,
            true,
            None,
        )
        .unwrap()
    });
    let exact = input.read_with(cx, |input, _| {
        let current = input.current_realization_ownership();
        let charge = page.retained_charge();
        RangeSurfaceCharge {
            bytes: current.owned_bytes + charge.bytes() - std::mem::size_of::<ObjectPage>()
                + charge.bytes(),
            items: current.owned_items + page.objects().len() + page.objects().len() + 1,
        }
    });
    input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = exact.bytes - 1;
        input.config.limits.max_surface_items = exact.items;
    });
    let rejected = input.update(cx, |input, cx| input.deliver_object_page(page, cx));
    let RangeTextInputError::ObjectResponseCapacity(page) = rejected.unwrap_err() else {
        panic!("typed object custody")
    };
    input.update(cx, |input, _| {
        assert!(input.dispatched_object_pages.contains(&page.key()));
        input.config.limits.max_surface_bytes = exact.bytes;
        input
            .admit_response_custody(
                super::super::response_custody::RangeResponseCustody::Object(page),
            )
            .unwrap();
        assert_eq!(input.response_custody.len(), 1);
    });
}

#[gpui::test]
fn disposed_late_text_and_object_responses_return_payload_without_reallocating(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let text_key = crate::PageRequestKey::adjacent(
        PageRequestId::new(182_400),
        binding().binding(),
        binding().revision(),
        PagePurpose::Restoration,
        ByteOffset::new(0),
        PageDirection::Forward,
        SOURCE.len() as u64,
    )
    .unwrap();
    let text = page_for(PageRequest::new(text_key), 182_400);
    let object_key = crate::ObjectRequestKey::new(
        ObjectRequestId::new(182_401),
        binding().binding(),
        binding().revision(),
        PresentationGeneration::new(1),
        ObjectPurpose::Restoration,
        ObjectDemandEnvelope::anchor(ByteOffset::new(0), None, ObjectDirection::Forward, 1, 4_096)
            .unwrap(),
    )
    .unwrap();
    let object = ObjectPage::new(
        ObjectPageId::new(182_401),
        object_key,
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
            assert_eq!(input.requests.capacity(), 0);
        })
    });
    let text = cx
        .update(|window, app| input.update(app, |input, cx| input.deliver_page(text, window, cx)))
        .unwrap_err();
    assert!(matches!(text, RangeTextInputError::PageResponseRejected(_)));
    let object = input
        .update(cx, |input, cx| input.deliver_object_page(object, cx))
        .unwrap_err();
    assert!(matches!(
        object,
        RangeTextInputError::ObjectResponseRejected(_)
    ));
    input.read_with(cx, |input, _| {
        let current = input.realization_diagnostics().current;
        assert_eq!(input.requests.capacity(), 0);
        assert_eq!(current.request_storage_bytes, 0);
        assert_eq!(current.request_storage_items, 0);
        assert_eq!(current.queued_requests, 0);
        assert_eq!(current.response_custody_bytes, 0);
        assert_eq!(current.response_custody_items, 0);
    });
}

#[gpui::test]
fn request_queue_exact_fit_and_full_bound_never_grow_backing(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let capacity = input.requests.capacity();
        let current = input.current_realization_ownership();
        let key = crate::PageRequestKey::adjacent(
            PageRequestId::new(182_500),
            binding().binding(),
            binding().revision(),
            PagePurpose::Viewport,
            ByteOffset::new(0),
            PageDirection::Forward,
            4,
        )
        .unwrap();
        input.config.limits.max_surface_bytes = current.owned_bytes - 1;
        assert!(matches!(
            input.push_request(RangeTextInputRequest::ReleasePage(key), cx),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        assert!(input.requests.is_empty());
        input.config.limits.max_surface_bytes = current.owned_bytes;
        input
            .push_request(RangeTextInputRequest::ReleasePage(key), cx)
            .unwrap();
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
        while input.requests.len() < capacity {
            input
                .push_request(RangeTextInputRequest::ReleasePage(key), cx)
                .unwrap();
        }
        assert_eq!(input.requests.capacity(), capacity);
        assert!(matches!(
            input.push_request(RangeTextInputRequest::ReleasePage(key), cx),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        assert_eq!(input.requests.len(), capacity);
        assert_eq!(input.requests.capacity(), capacity);
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .request_storage_items,
            capacity
        );
    });
}

#[gpui::test]
fn rejected_target_and_configuration_attempts_refund_shared_frame_credit(
    cx: &mut gpui::TestAppContext,
) {
    for reject_target in [false, true] {
        let (input, cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
        });
        drive_initial_surface(&input, cx);
        input.update(cx, |input, cx| {
            input.config.limits.max_realization_work_per_frame = 1;
            input.begin_realization_frame();
            let current = input.current_realization_ownership();
            input.config.limits.max_surface_bytes = current.owned_bytes - 1;
            let rejected = if reject_target {
                input.request_absolute_scroll(px(16.), cx)
            } else {
                input.set_presentation_generation(PresentationGeneration::new(2), cx)
            };
            assert!(matches!(
                rejected,
                Err(RangeTextInputError::SurfaceCapacity)
            ));
            assert_eq!(input.last_realization_step.spent, 0);
            assert_eq!(input.last_realization_step.remaining, 1);
            input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
            if reject_target {
                input
                    .set_presentation_generation(PresentationGeneration::new(3), cx)
                    .unwrap();
            } else {
                input.request_absolute_scroll(px(32.), cx).unwrap();
            }
            assert_eq!(input.last_realization_step.spent, 1);
            assert_eq!(input.last_realization_step.remaining, 0);
        });
    }
}

#[gpui::test]
fn noncommitted_history_and_mutation_settlement_share_frame_credit(cx: &mut gpui::TestAppContext) {
    let (history_input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(512 * 1024 * 1024, 1_048_576), window, cx).unwrap()
    });
    drive_initial_surface(&history_input, cx);
    let frontier = crate::RangeHistoryFrontier {
        binding: binding(),
        id: 1,
        undo_available: true,
        redo_available: false,
    };
    let intent = admit_history(&history_input, cx, frontier, crate::MutationKind::Undo);
    cx.update(|window, app| {
        history_input.update(app, |input, cx| {
            input.config.limits.max_realization_work_per_frame = 1;
            input.begin_realization_frame();
            input.spend_realization_credit();
            assert!(matches!(
                input.settle_history(intent, RangeHistoryOutcome::Rejected, window, cx),
                Err(RangeTextInputError::Busy)
            ));
            assert!(input.pending_history.is_some());
            input.begin_realization_frame();
            assert_eq!(
                input
                    .settle_history(intent, RangeHistoryOutcome::Rejected, window, cx)
                    .unwrap(),
                RangeHistorySettlement::Current(RangeHistoryOutcome::Rejected)
            );
            assert_eq!(input.last_realization_step.spent, 1);
        })
    });

    let (mutation_input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(512 * 1024 * 1024, 1_048_576), window, cx).unwrap()
    });
    drive_initial_surface(&mutation_input, cx);
    let (key, _) = drive_local_insert_to_commit_pending(&mutation_input, cx);
    cx.update(|window, app| {
        mutation_input.update(app, |input, cx| {
            input.config.limits.max_realization_work_per_frame = 1;
            input.begin_realization_frame();
            input.spend_realization_credit();
            assert!(matches!(
                input.settle_mutation(key, crate::MutationOutcome::Rejected, window, cx),
                Err(RangeTextInputError::Busy)
            ));
            assert_eq!(input.edits.active_key(), Some(key));
            input.begin_realization_frame();
            input
                .settle_mutation(key, crate::MutationOutcome::Rejected, window, cx)
                .unwrap();
            assert_eq!(input.last_realization_step.spent, 1);
        })
    });
}

#[gpui::test]
fn geometry_text_and_object_capacity_return_typed_custody_and_retry(cx: &mut gpui::TestAppContext) {
    let (text_input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let text_request = match text_input
        .update(cx, |input, _| input.take_request())
        .unwrap()
    {
        RangeTextInputRequest::Page(request) => request,
        other => panic!("geometry text request: {other:?}"),
    };
    let text_page = page_for(text_request, 182_600);
    let text_exact = text_input.read_with(cx, |input, _| {
        let current = input.current_realization_ownership();
        let charge = text_page.retained_charge();
        RangeSurfaceCharge {
            bytes: current.owned_bytes + charge.bytes() - std::mem::size_of::<RangePage>()
                + charge.bytes(),
            items: current.owned_items + charge.items() - 1 + charge.items(),
        }
    });
    text_input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = text_exact.bytes - 1;
        input.config.limits.max_surface_items = text_exact.items;
    });
    let text_page = match cx
        .update(|window, app| {
            text_input.update(app, |input, cx| input.deliver_page(text_page, window, cx))
        })
        .unwrap_err()
    {
        RangeTextInputError::PageResponseCapacity(page) => page,
        other => panic!("typed geometry text custody: {other:?}"),
    };
    text_input.update(cx, |input, _| {
        assert!(input.dispatched_pages.contains(&text_page.key()));
        input.config.limits.max_surface_bytes = text_exact.bytes;
    });
    cx.update(|window, app| {
        text_input.update(app, |input, cx| {
            input.begin_realization_frame();
            input.deliver_page(text_page, window, cx).unwrap();
        })
    });

    let source = "line\n".repeat(24);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(182_601),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 24),
    );
    let (object_input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let key = stage_terminal_target_object_response(&object_input, cx, &source);
    let object_page = empty_terminal_object_response(key);
    let object_exact = object_input.read_with(cx, |input, _| {
        let current = input.current_realization_ownership();
        let charge = object_page.retained_charge();
        RangeSurfaceCharge {
            bytes: current.owned_bytes + charge.bytes() - std::mem::size_of::<ObjectPage>()
                + charge.bytes(),
            items: current.owned_items + charge.objects() + charge.objects() + 1,
        }
    });
    object_input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = object_exact.bytes - 1;
        input.config.limits.max_surface_items = object_exact.items;
    });
    let object_page = match cx
        .update(|window, app| {
            object_input.update(app, |input, cx| {
                input.deliver_object_page_in_window(object_page, window, cx)
            })
        })
        .unwrap_err()
    {
        RangeTextInputError::ObjectResponseCapacity(page) => page,
        other => panic!("typed geometry object custody: {other:?}"),
    };
    object_input.update(cx, |input, _| {
        assert!(input.dispatched_object_pages.contains(&object_page.key()));
        input.config.limits.max_surface_bytes = object_exact.bytes;
    });
    cx.update(|window, app| {
        object_input.update(app, |input, cx| {
            input.begin_realization_frame();
            input
                .deliver_object_page_in_window(object_page, window, cx)
                .unwrap();
        })
    });
}
