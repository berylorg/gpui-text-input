use super::*;

#[path = "view_demand_edit_lifecycle/resources.rs"]
mod resources;

#[derive(Clone, Copy)]
enum AdmissionProbe {
    Ordinary,
    Capacity {
        limit: crate::RangeSurfaceCharge,
        refuse: bool,
    },
    Exhaustion,
}

fn logical_object(
    pending: &super::super::geometry::PendingGeometryObject,
) -> (crate::GeometryJobKey, ObjectPurpose, ObjectDemandEnvelope) {
    assert!(pending.request.is_none());
    let GeometryObjectWait::Unstarted { purpose, demand } = pending.wait else {
        panic!("original unstarted object intent")
    };
    (pending.job, purpose, demand)
}

fn logical_text(
    pending: &super::super::geometry::PendingGeometryPage,
) -> (crate::GeometryJobKey, PagePurpose, PageDemandEnvelope) {
    assert!(pending.request.is_none());
    let GeometryPageWait::Unstarted { purpose, demand } = pending.wait else {
        panic!("original unstarted text intent")
    };
    (pending.job, purpose, demand)
}

fn begin_local_edit(
    input: &mut RangeTextInput,
    cx: &mut gpui::Context<RangeTextInput>,
) -> crate::MutationKey {
    let at = SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::NoObjects);
    let positions = crate::MutationPositions::collapsed(at);
    let proposal = crate::MutationProposal::new(
        crate::MutationKey::new(
            input.config.binding.binding(),
            input.config.binding.revision(),
            crate::OperationId::new(1),
        ),
        crate::MutationKind::Edit,
        positions,
        crate::SourceRange::new(at, at).unwrap(),
        0,
    );
    input
        .begin_local_mutation(proposal, vec![], positions, cx)
        .unwrap();
    input.edits.active_key().unwrap()
}

fn long_config() -> RangeTextInputConfig {
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(192, 1),
    );
    configuration
}

fn empty_objects(request: crate::ObjectRequest, id: u64) -> ObjectPage {
    ObjectPage::new(
        ObjectPageId::new(id),
        request.key(),
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap()
}

#[gpui::test]
fn active_edit_coalesces_environment_without_changing_fifo_or_dispatch_custody(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let key = begin_local_edit(input, cx);
        let before = (input.next_id, input.geometry.counts(), fingerprint(input).requests, input.dispatched_mutations.len());
        for width in [140., 160., 180.] {
            let mut layout = input.config.layout.clone();
            layout.wrap_width = px(width);
            input.set_layout(layout, input.config.style.clone(), cx).unwrap();
            input.set_realization_viewport_extent(px(width / 2.), cx).unwrap();
            input.set_presentation_generation(PresentationGeneration::new(2), cx).unwrap();
            input.begin_realization_frame();
            input.service_pending_target_intent(cx).unwrap();
            input.service_pending_configuration_intent(cx).unwrap();
        }
        assert_eq!((input.next_id, input.geometry.counts(), fingerprint(input).requests, input.dispatched_mutations.len()), before);
        assert_eq!(input.pending_layout_intent.as_ref().unwrap().layout.wrap_width, px(180.));
        assert_eq!(input.pending_target_intent.unwrap().desired.viewport_extent, px(90.));
        let fingerprint_before = fingerprint(input);
        assert!(input.take_request_if(|request| { assert!(matches!(request, RangeTextInputRequest::MutationBegin(begin) if begin.proposal().key() == key)); false }).is_none());
        assert_eq!(fingerprint(input), fingerprint_before);
        assert!(matches!(input.take_request_if(|_| true), Some(RangeTextInputRequest::MutationBegin(begin)) if begin.proposal().key() == key));
        input.reject_mutation_preflight(key, cx).unwrap();
        input.begin_realization_frame();
        input.service_pending_configuration_intent(cx).unwrap();
        input.service_pending_configuration_intent(cx).unwrap();
        assert!(input.pending_layout_intent.is_none());
        assert!(input.pending_target_intent.is_none());
        assert!(input.pending_presentation_intent.is_none());
        assert_eq!(input.config.layout.wrap_width, px(180.));
        assert_eq!(input.desired.viewport_extent, px(90.));
        assert_eq!(input.config.presentation_generation, PresentationGeneration::new(2));
        assert!(input.realization_diagnostics().current.owned_bytes <= input.config.limits.max_surface_bytes);
    });
}

#[gpui::test]
fn admitted_text_response_retains_unstarted_object_until_noncommit(cx: &mut gpui::TestAppContext) {
    let source = "x".repeat(192);
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(long_config(), window, cx).unwrap());
    let Some(RangeTextInputRequest::Page(request)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("original text demand")
    };
    let key = input.update(cx, begin_local_edit);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_page(page_for_source(request, 1, &source), window, cx)
                .unwrap();
            assert!(!input.dispatched_pages.contains(&request.key()));
            let pending = input.pending_geometry_object.as_ref().unwrap();
            assert!(matches!(pending.wait, GeometryObjectWait::Unstarted { .. }));
            let successor = logical_object(pending);
            assert!(
                !input
                    .requests
                    .iter()
                    .any(|request| matches!(request, RangeTextInputRequest::ObjectPage(_)))
            );
            input.set_enabled(false, cx);
            input
                .service_geometry_until_external_boundary(window, cx)
                .unwrap();
            assert!(matches!(
                input.pending_geometry_object.as_ref().unwrap().wait,
                GeometryObjectWait::Unstarted { .. }
            ));
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::MutationBegin(_))
            ));
            input.reject_mutation_preflight(key, cx).unwrap();
            input.begin_realization_frame();
            input
                .service_geometry_until_external_boundary(window, cx)
                .unwrap();
            let admitted = input
                .requests
                .iter()
                .find_map(|request| match request {
                    RangeTextInputRequest::ObjectPage(request)
                        if (request.key().purpose(), request.key().demand())
                            == (successor.1, successor.2) =>
                    {
                        Some(request.key())
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(input.active_geometry, Some(successor.0));
            assert!(
                input
                    .object_residency
                    .pending_requests()
                    .any(|key| key == admitted)
            );
            assert!(
                !input
                    .requests
                    .iter()
                    .any(|request| matches!(request, RangeTextInputRequest::CancelObjectPage(_)))
            );
        })
    });
}

#[gpui::test]
fn admitted_object_response_retains_unstarted_text_and_rebind_retires_without_cancel(
    cx: &mut gpui::TestAppContext,
) {
    let source = "x".repeat(192);
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(long_config(), window, cx).unwrap());
    let Some(RangeTextInputRequest::Page(request)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("original text demand")
    };
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_page(page_for_source(request, 1, &source), window, cx)
                .unwrap()
        })
    });
    let object = loop {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::ObjectPage(request)) => break request,
            Some(RangeTextInputRequest::ReleasePage(_)) => {}
            other => panic!("original object demand: {other:?}"),
        }
    };
    let key = input.update(cx, begin_local_edit);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(empty_objects(object, 2), window, cx)
                .unwrap();
            let pending = input.pending_geometry_page.as_ref().unwrap();
            assert!(matches!(pending.wait, GeometryPageWait::Unstarted { .. }));
            let successor = logical_text(pending);
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::MutationBegin(_))
            ));
            let evidence = input.request_mutation_evidence(key).unwrap();
            let super::super::mutation_evidence::LocalMutationEvidence::Finish(finish) =
                input.local_mutation_evidence(evidence, cx).unwrap()
            else {
                panic!("empty original evidence closure")
            };
            input
                .submit_mutation_evidence_finish(evidence, finish)
                .unwrap();
            input.accept_mutation_preflight(key, cx).unwrap();
            let pass = input.mutation_restart(key).unwrap();
            input.acknowledge_mutation_restart(pass, cx).unwrap();
            while let Some(request) = input.take_request() {
                assert!(!matches!(request, RangeTextInputRequest::Page(_)));
                if matches!(request, RangeTextInputRequest::MutationFinishInput(_)) {
                    input.accept_mutation_finish(key, cx).unwrap();
                }
            }
            let at = SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::NoObjects);
            let (binding, text, objects) = admitted_successor_sources(&source, 2, &[at]);
            input.begin_realization_frame();
            input
                .settle_committed_mutation(
                    key,
                    binding,
                    crate::MutationPositions::collapsed(at),
                    &text,
                    &objects,
                    window,
                    cx,
                )
                .unwrap();
            assert!(
                input
                    .pending_geometry_page
                    .as_ref()
                    .is_none_or(|pending| pending.job != successor.0)
            );
            assert!(
                !input
                    .requests
                    .iter()
                    .any(|request| matches!(request, RangeTextInputRequest::CancelPage(_)))
            );
            assert_eq!(input.config.binding, binding);
        })
    });
}

#[gpui::test]
fn unstarted_successor_capacity_refusal_preserves_original_response_and_edit(
    cx: &mut gpui::TestAppContext,
) {
    let source = "x".repeat(192);
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(long_config(), window, cx).unwrap());
    let Some(RangeTextInputRequest::Page(request)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("original text demand")
    };
    let key = input.update(cx, begin_local_edit);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let page = page_for_source(request, 1, &source);
            let before = fingerprint(input);
            let prepare = |input: &RangeTextInput| {
                let admission = input.residency.prepare_admit(page.clone()).unwrap();
                let geometry = input
                    .geometry
                    .prepare_target_page(
                        input.active_geometry.unwrap(),
                        admission.page(),
                        window.text_system(),
                        input.target_response_successor().unwrap(),
                    )
                    .unwrap();
                input.prepare_nonterminal_response_publication(
                    geometry,
                    Some(admission),
                    None,
                    None,
                    None,
                    Some(request.key()),
                    None,
                    Some(request.key()),
                    None,
                    None,
                )
            };
            let (mut low, mut high) = (1, input.config.limits.max_surface_bytes);
            assert!(prepare(input).is_ok());
            while low < high {
                let mid = low + (high - low) / 2;
                input.config.limits.max_surface_bytes = mid;
                if prepare(input).is_ok() {
                    high = mid;
                } else {
                    low = mid + 1;
                }
            }
            input.config.limits.max_surface_bytes = low - 1;
            assert!(matches!(
                prepare(input),
                Err(RangeTextInputError::SurfaceCapacity)
            ));
            input.config.limits.max_surface_bytes = low;
            assert!(prepare(input).is_ok());
            assert_eq!(fingerprint(input), before);
            assert_eq!(input.edits.active_key(), Some(key));
            input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
            input.deliver_page(page, window, cx).unwrap();
            assert!(matches!(
                input.pending_geometry_object.as_ref().unwrap().wait,
                GeometryObjectWait::Unstarted { .. }
            ));
            let successor = logical_object(input.pending_geometry_object.as_ref().unwrap());
            let retired = input.dispose(window, cx);
            assert!(
                !input
                    .requests
                    .iter()
                    .any(|request| matches!(request, RangeTextInputRequest::CancelObjectPage(_)))
            );
            assert!(input.pending_geometry_object.is_none());
            assert!(
                !retired
                    .iter()
                    .any(|request| matches!(request, RangeTextInputRequest::CancelObjectPage(_)))
            );
            assert_ne!(input.active_geometry, Some(successor.0));
        })
    });
}

#[gpui::test]
fn partial_object_response_retains_exact_cursor_without_admitting_successor_during_edit(
    cx: &mut gpui::TestAppContext,
) {
    let source = "x".repeat(192);
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(long_config(), window, cx).unwrap());
    let Some(RangeTextInputRequest::Page(request)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("original text demand")
    };
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_page(page_for_source(request, 1, &source), window, cx)
                .unwrap()
        })
    });
    let object = loop {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::ObjectPage(request)) => break request,
            Some(RangeTextInputRequest::ReleasePage(_)) => {}
            other => panic!("original object demand: {other:?}"),
        }
    };
    let key = input.update(cx, begin_local_edit);
    cx.update(|window, app| input.update(app, |input, cx| {
        let cursor = crate::ObjectCursor::new(ByteOffset::new(0), InlineObjectOrder::new(1), InlineObjectId::new(1));
        let presentation = InlineObjectPresentation::new(1, SharedString::new_static("object"), px(18.), px(16.), px(12.), None, 0, true).unwrap();
        let page = ObjectPage::new(ObjectPageId::new(2), object.key(), vec![InlineObjectFact::new(cursor.id(), cursor.anchor(), cursor.order(), "object", presentation)], ObjectPageEdgeFact::EnvelopeBoundary, ObjectPageEdgeFact::Continues(cursor), false, Some(cursor)).unwrap();
        input.deliver_object_page_in_window(page, window, cx).unwrap();
        let pending = input.pending_geometry_object.as_ref().unwrap();
        assert!(matches!(pending.wait, GeometryObjectWait::Unstarted { .. }));
        let successor = logical_object(pending);
        assert!(matches!(successor.2, ObjectDemandEnvelope::Range { cursor: Some(value), .. } if value == cursor));
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::MutationBegin(_))));
        input.reject_mutation_preflight(key, cx).unwrap();
        input.begin_realization_frame();
        input.service_geometry_until_external_boundary(window, cx).unwrap();
        assert!(input.requests.iter().any(|request| matches!(request, RangeTextInputRequest::ObjectPage(request) if (request.key().purpose(), request.key().demand()) == (successor.1, successor.2))));
    }));
}

#[gpui::test]
fn clipboard_continuation_does_not_strand_deferred_geometry_text_after_noncommit(
    cx: &mut gpui::TestAppContext,
) {
    copy_with_delayed_text(cx, AdmissionProbe::Ordinary);
}

fn copy_with_delayed_text(
    cx: &mut gpui::TestAppContext,
    probe: AdmissionProbe,
) -> crate::RangeSurfaceCharge {
    let source = "x".repeat(1024);
    let mut configuration = long_config();
    configuration.binding = RangeBinding::new(
        configuration.binding.binding(),
        configuration.binding.revision(),
        LogicalExtent::new(source.len() as u64, 1),
    );
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);
    input.update(cx, |input, cx| {
        input
            .publish_optional_source_selection(
                Some(RangeSourceSelection {
                    anchor: SourcePosition::new(
                        ByteOffset::new(0),
                        crate::InlineObjectGap::NoObjects,
                    ),
                    head: SourcePosition::new(
                        ByteOffset::new(source.len() as u64),
                        crate::InlineObjectGap::NoObjects,
                    ),
                }),
                None,
                None,
                cx,
            )
            .unwrap();
    });
    drive_surface_for_source_from(&input, cx, &source, 10_000);
    input.update(cx, |input, cx| {
        input.request_absolute_scroll(px(240.), cx).unwrap()
    });
    let object = (20_000..20_128)
        .find_map(|id| {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
            match input.update(cx, |input, _| input.take_request()) {
                Some(RangeTextInputRequest::ObjectPage(request)) => Some(request),
                Some(RangeTextInputRequest::Page(request)) => {
                    cx.update(|window, app| {
                        input.update(app, |input, cx| {
                            input
                                .deliver_page(page_for_source(request, id, &source), window, cx)
                                .unwrap()
                        })
                    });
                    None
                }
                Some(
                    RangeTextInputRequest::ReleasePage(_)
                    | RangeTextInputRequest::ReleaseObjectPage(_)
                    | RangeTextInputRequest::CancelPage(_)
                    | RangeTextInputRequest::CancelObjectPage(_),
                )
                | None => None,
                other => panic!("ordinary target producer returned {other:?}"),
            }
        })
        .expect("ordinary scroll target admits original object work");
    let edit = input.update(cx, begin_local_edit);
    let reserved = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(empty_objects(object, 30_000), window, cx)
                .unwrap();
            let pending = input
                .pending_geometry_page
                .as_ref()
                .expect("target retains its next text demand");
            assert!(matches!(pending.wait, GeometryPageWait::Unstarted { .. }));
            let reserved = logical_text(pending);
            assert_eq!(input.residency.pending_requests().len(), 0);
            let surface = input.surface().unwrap();
            assert_eq!(surface.selection().anchor.byte_offset.get(), 0);
            assert_eq!(
                surface.selection().head.byte_offset.get(),
                source.len() as u64
            );
            assert_eq!(surface.binding(), input.config.binding);
            input
                .begin_clipboard(crate::ClipboardKind::Copy, cx)
                .unwrap();
            reserved
        })
    });
    let mut delivered_clipboard_text = false;
    let later_clipboard = (40_000..40_256)
        .find_map(|id| {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
            match input.update(cx, |input, _| input.take_request()) {
                Some(RangeTextInputRequest::MutationBegin(begin)) => {
                    assert_eq!(begin.proposal().key(), edit);
                    None
                }
                Some(RangeTextInputRequest::Page(request)) => {
                    assert_eq!(request.key().purpose(), PagePurpose::Clipboard);
                    assert!(request.key().id().get() > object.key().id().get());
                    if delivered_clipboard_text {
                        Some(request.key())
                    } else {
                        cx.update(|window, app| {
                            input.update(app, |input, cx| {
                                input
                                    .deliver_page(page_for_source(request, id, &source), window, cx)
                                    .unwrap()
                            })
                        });
                        delivered_clipboard_text = true;
                        None
                    }
                }
                Some(RangeTextInputRequest::ObjectPage(request)) => {
                    assert_eq!(request.key().purpose(), ObjectPurpose::Clipboard);
                    cx.update(|window, app| {
                        input.update(app, |input, cx| {
                            input
                                .deliver_object_page_in_window(
                                    empty_objects(request, id),
                                    window,
                                    cx,
                                )
                                .unwrap()
                        })
                    });
                    None
                }
                Some(
                    RangeTextInputRequest::ReleasePage(_)
                    | RangeTextInputRequest::ReleaseObjectPage(_),
                )
                | None => None,
                other => panic!("original clipboard continuation returned {other:?}"),
            }
        })
        .expect("original clipboard producer advances to a later text demand");
    cx.update(|window, app| input.update(app, |input, cx| {
        assert!(input.residency.pending_requests().any(|key| key == later_clipboard));
        assert!(input.dispatched_pages.contains(&later_clipboard));
        assert!(input.residency.pending_requests().len() < input.config.residency_limits.max_pending_requests());
        assert!(input.realization_diagnostics().current.owned_bytes < input.config.limits.max_surface_bytes);
        assert_eq!(input.config.binding, input.surface().unwrap().binding());
        input.reject_mutation_preflight(edit, cx).unwrap();
        assert!(input.edits.active_key().is_none());
        input.begin_realization_frame();
        let original_limits = crate::RangeSurfaceCharge { bytes: input.config.limits.max_surface_bytes, items: input.config.limits.max_surface_items };
        let original_next_id = input.next_id;
        input.realization_high_water.owned_bytes = 0;
        input.realization_high_water.owned_items = 0;
        match probe {
            AdmissionProbe::Capacity { limit, .. } => { input.config.limits.max_surface_bytes = limit.bytes; input.config.limits.max_surface_items = limit.items; }
            AdmissionProbe::Exhaustion => input.next_id = u64::MAX,
            AdmissionProbe::Ordinary => {},
        }
        let before_attempt = fingerprint(input);
        let resumed = input.service_geometry_until_external_boundary(window, cx);
        if matches!(probe, AdmissionProbe::Capacity { refuse: true, .. } | AdmissionProbe::Exhaustion) {
            match probe {
                AdmissionProbe::Exhaustion => assert!(matches!(resumed, Err(RangeTextInputError::Stale))),
                _ => assert!(matches!(resumed, Err(RangeTextInputError::SurfaceCapacity))),
            }
            assert_eq!(fingerprint(input), before_attempt);
            assert_eq!(logical_text(input.pending_geometry_page.as_ref().unwrap()), reserved);
            assert!(input.residency.pending_requests().any(|key| key == later_clipboard));
            assert!(input.dispatched_pages.contains(&later_clipboard));
            input.config.limits.max_surface_bytes = original_limits.bytes;
            input.config.limits.max_surface_items = original_limits.items;
            input.next_id = original_next_id;
            input.begin_realization_frame();
            input.service_geometry_until_external_boundary(window, cx).unwrap();
        } else {
            assert!(resumed.is_ok(), "original deferred geometry text demand must resume after noncommit: result={resumed:?}, logical={reserved:?}, later_clipboard={later_clipboard:?}");
        }
        let admitted = input.requests.iter().find_map(|request| match request { RangeTextInputRequest::Page(request) if (request.key().purpose(), request.key().demand()) == (reserved.1, reserved.2) => Some(request.key()), _ => None }).unwrap();
        assert_eq!(input.active_geometry, Some(reserved.0));
        assert!(admitted.id() > later_clipboard.id());
        assert!(input.residency.pending_requests().any(|key| key == admitted));
        assert!(!input.dispatched_pages.contains(&admitted));
        let diagnostics = input.realization_diagnostics();
        assert!(diagnostics.current.owned_bytes <= input.config.limits.max_surface_bytes);
        assert!(diagnostics.current.owned_items <= input.config.limits.max_surface_items);
        crate::RangeSurfaceCharge { bytes: diagnostics.high_water.owned_bytes, items: diagnostics.high_water.owned_items }
    }))
}
