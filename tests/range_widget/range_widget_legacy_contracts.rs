use super::*;

fn begin_request(requests: &[RangeTextInputRequest]) -> gpui_text_input::MutationBeginRequest {
    requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationBegin(begin) => Some(*begin),
            _ => None,
        })
        .expect("mutation begin")
}

#[cfg(feature = "test-support")]
fn page_for_with_reserved_atom_capacity(
    source: &str,
    id: u64,
    request: gpui_text_input::PageRequest,
    atom_capacity: usize,
) -> RangePage {
    let key = request.key();
    let base = page_for(source, id, request);
    RangePage::new(
        base.id(),
        key,
        base.range(),
        base.text().to_owned(),
        Vec::with_capacity(atom_capacity),
        base.preceding(),
        base.following(),
        base.end_of_source(),
    )
    .unwrap()
}

fn accept_local_to_commit(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    begin: gpui_text_input::MutationBeginRequest,
) -> gpui_text_input::MutationFinishInput {
    input.update(cx, |input, cx| {
        input
            .accept_mutation_preflight(begin.proposal().key(), cx)
            .unwrap()
    });
    let requests = drive_pages(input, cx, source);
    let finish = requests
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::MutationFinishInput(finish) => Some(*finish),
            _ => None,
        })
        .expect("authenticated mutation finish");
    assert!(requests.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::MutationProposalPage(page)
            if page.page().key().key() == begin.proposal().key()
    )));
    input.update(cx, |input, cx| {
        input
            .accept_mutation_finish(begin.proposal().key(), cx)
            .unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, _| input.take_request()),
        Some(RangeTextInputRequest::MutationCommit(commit))
            if commit.key() == begin.proposal().key()
    ));
    finish
}

#[gpui::test]
fn live_range_widget_builds_one_exact_surface_and_drives_ime_mutation(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "hello\nworld";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        let surface = input.surface().expect("coherent surface");
        assert_eq!(surface.binding(), binding(source, 1));
        assert_eq!(surface.caret().byte_offset.get(), 0);
        assert!(!surface.fragments().is_empty());
    });

    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0]);
    });
    cx.simulate_input("!");
    let requests = drive_pages(&input, cx, source);
    let begin = begin_request(&requests);
    let finish = accept_local_to_commit(&input, cx, source, begin);
    assert_eq!(finish.intended().caret().byte_offset, ByteOffset::new(1));
}

#[gpui::test]
fn nonresident_platform_replacement_resolves_exact_range_before_preflight(
    cx: &mut gpui::TestAppContext,
) {
    let source = "abcdefghij".repeat(20);
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(&source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, &source, 1, &[150, 160]);
    });
    input.update(cx, |input, cx| {
        input
            .replace_platform_range(150..160, "X".to_owned(), cx)
            .unwrap();
    });
    let requests = drive_pages(&input, cx, &source);
    let begin = begin_request(&requests);
    assert_eq!(
        begin.proposal().replacement(),
        ordinary_range(ByteRange::from_u64(150, 160).unwrap())
    );
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), binding(&source, 1))
    });
}

#[gpui::test]
fn nonresident_marked_replacement_preserves_exact_composition_and_selection(
    cx: &mut gpui::TestAppContext,
) {
    assert_nonresident_marked_replacement(cx, false);
}

#[cfg(feature = "test-support")]
#[gpui::test]
fn superseded_marked_index_capacity_denial_settles_and_publishes(cx: &mut gpui::TestAppContext) {
    assert_nonresident_marked_replacement(cx, true);
}

fn assert_nonresident_marked_replacement(
    cx: &mut gpui::TestAppContext,
    deny_superseded_candidate: bool,
) {
    let source = "abcdefghij".repeat(20);
    let inserted = "\u{00e9}\u{1f642}";
    let configuration = config(&source, 1);
    #[cfg(feature = "test-support")]
    let configuration = if deny_superseded_candidate {
        let mut configuration = configuration;
        configuration.residency_limits =
            ResidencyLimits::new(256, 128 * 1024, 256, 8 * 1024).unwrap();
        configuration.limits =
            RangeTextInputLimits::new(4 * 1024 * 1024, 32768, 8, px(80.), 32, 32, px(16.)).unwrap();
        configuration
    } else {
        configuration
    };
    #[cfg(not(feature = "test-support"))]
    assert!(!deny_superseded_candidate);
    let (input, cx) = cx.add_window_view(move |window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, &source, 1, &[150, 160]);
    });

    input.update(cx, |input, cx| input.set_read_only(true, cx));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_and_mark_text_in_range(Some(150..160), inserted, Some(1..3), window, cx);
        })
    });
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.update(cx, |input, cx| input.set_read_only(false, cx));

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_and_mark_text_in_range(Some(150..160), inserted, Some(1..3), window, cx);
        })
    });
    let begin = begin_request(&drive_pages(&input, cx, &source));
    assert_eq!(
        begin.proposal().replacement(),
        ordinary_range(ByteRange::from_u64(150, 160).unwrap())
    );
    let finish = accept_local_to_commit(&input, cx, &source, begin);
    assert_eq!(finish.intended().caret().byte_offset, ByteOffset::new(156));
    assert_eq!(finish.intended().selection_anchor(), ordinary_position(152));
    assert_eq!(finish.intended().selection_head(), ordinary_position(156));
    let successor = format!("{}{}{}", &source[..150], inserted, &source[160..]);
    let intended = finish.intended();
    let positions = [
        intended.caret(),
        intended.selection_anchor(),
        intended.selection_head(),
    ];
    let (text, objects) = admitted_sources(&successor, 2, &positions);
    let settlement = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_committed_mutation(
                    begin.proposal().key(),
                    binding(&successor, 2),
                    intended,
                    &text,
                    &objects,
                    window,
                    cx,
                )
                .unwrap()
        })
    });
    assert!(matches!(
        settlement,
        gpui_text_input::MutationSettlement::Current(
            gpui_text_input::MutationOutcome::Committed(commit)
        ) if commit.binding() == binding(&successor, 2)
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.surface().unwrap().binding(), binding(&source, 1));
        assert!(!input.is_surface_current_and_interactive());
    });

    let rejection_baseline = input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.response_rejection_count, 0);
        assert_eq!(diagnostics.last_response_rejection, None);
        assert_eq!(diagnostics.last_response_rejection_stage, None);
        (
            diagnostics.response_rejection_count,
            diagnostics.last_response_rejection,
            diagnostics.last_response_rejection_stage,
        )
    });
    let mut saw_revision_two_request = false;
    let mut reached_quiescence = false;
    let mut last_response = None;
    let mut responses = Vec::new();
    let mut page_dispatches = Vec::new();
    let mut object_dispatches = Vec::new();
    let mut page_releases = Vec::new();
    let mut object_releases = Vec::new();
    let mut page_cancellations = Vec::new();
    let mut object_cancellations = Vec::new();
    let mut superseded_index_key = None;
    #[cfg(feature = "test-support")]
    let mut exact_processing_limit = None;
    for _ in 0..512 {
        let request = input.update(cx, |input, _| input.take_request());
        let had_request = request.is_some();
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                assert_eq!(request.key().binding(), BindingId::new(17));
                assert_eq!(request.key().revision(), SourceRevision::new(2));
                saw_revision_two_request = true;
                page_dispatches.push(request.key());
                if request.key().purpose() == gpui_text_input::PagePurpose::GeometryIndex
                    && superseded_index_key.is_none()
                {
                    superseded_index_key = Some(request.key());
                }
                #[cfg(feature = "test-support")]
                let deny_this_candidate = deny_superseded_candidate
                    && request.key() == superseded_index_key.expect("index key was captured");
                #[cfg(feature = "test-support")]
                let page = if deny_this_candidate {
                    page_for_with_reserved_atom_capacity(
                        &successor,
                        request.key().id().get(),
                        request,
                        2048,
                    )
                } else {
                    page_for(&successor, request.key().id().get(), request)
                };
                #[cfg(not(feature = "test-support"))]
                let page = page_for(&successor, request.key().id().get(), request);
                last_response = Some(format!("page {:?}", request.key()));
                responses.push(last_response.clone().unwrap());
                #[cfg(feature = "test-support")]
                let response_items = page.retained_charge().items();
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        #[cfg(feature = "test-support")]
                        if deny_this_candidate {
                            let diagnostics = input.realization_diagnostics();
                            let exact_processing_items = diagnostics
                                .current
                                .owned_items
                                .checked_add(
                                    response_items.checked_mul(2).expect(
                                        "bounded response processing remains representable",
                                    ),
                                )
                                .and_then(|items| items.checked_sub(1))
                                .expect("response processing owns its embedded page record once");
                            assert!(
                                diagnostics.high_water.owned_items <= diagnostics.max_surface_items
                            );
                            input
                                .lower_max_surface_items_for_test(
                                    std::num::NonZeroUsize::new(exact_processing_items).unwrap(),
                                )
                                .unwrap();
                            exact_processing_limit = Some(exact_processing_items);
                            input.deliver_page(page, window, cx).unwrap();
                            let denied = input.realization_diagnostics();
                            assert_eq!(denied.max_surface_items, exact_processing_items);
                            assert!(denied.current.owned_items <= exact_processing_items);
                            assert!(denied.high_water.owned_items <= exact_processing_items);
                            assert_eq!(denied.current.active_geometry_jobs, 1);
                            assert_eq!(denied.current.candidates, 1);
                            assert_eq!(input.surface().unwrap().binding(), binding(&source, 1));
                            assert!(!input.is_surface_current_and_interactive());
                            return;
                        }
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                assert_eq!(request.key().binding(), BindingId::new(17));
                assert_eq!(request.key().revision(), SourceRevision::new(2));
                saw_revision_two_request = true;
                object_dispatches.push(request.key());
                last_response = Some(format!("object page {:?}", request.key()));
                responses.push(last_response.clone().unwrap());
                let page = restoration_object_page(request, &[], request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(key)) => page_releases.push(key),
            Some(RangeTextInputRequest::CancelPage(key)) => page_cancellations.push(key),
            Some(RangeTextInputRequest::ReleaseObjectPage(key)) => object_releases.push(key),
            Some(RangeTextInputRequest::CancelObjectPage(key)) => object_cancellations.push(key),
            Some(request) => panic!("unexpected marked-successor request: {request:?}"),
            None => {}
        }
        input.read_with(cx, |input, _| {
            let diagnostics = input.realization_diagnostics();
            assert_eq!(
                (
                    diagnostics.response_rejection_count,
                    diagnostics.last_response_rejection,
                    diagnostics.last_response_rejection_stage,
                ),
                rejection_baseline,
                "marked-successor response {last_response:?} was terminally rejected after {responses:?}: {diagnostics:?}"
            );
        });
        if input.read_with(cx, |input, _| input.is_quiescent()) {
            reached_quiescence = true;
            break;
        }
        if !had_request {
            cx.update(|window, app| window.draw(app).clear());
            cx.run_until_parked();
        }
    }
    assert!(saw_revision_two_request);
    let superseded_index_key = superseded_index_key.expect("revision-two index request dispatched");
    #[cfg(feature = "test-support")]
    assert_eq!(exact_processing_limit.is_some(), deny_superseded_candidate);
    assert!(
        reached_quiescence,
        "marked-successor drive exhausted its 512-step bound: {:?}",
        input.read_with(cx, |input, _| input.realization_diagnostics())
    );
    assert_eq!(
        page_cancellations
            .iter()
            .filter(|key| **key == superseded_index_key)
            .count(),
        0
    );
    assert_eq!(
        page_releases
            .iter()
            .filter(|key| **key == superseded_index_key)
            .count(),
        1
    );
    for key in page_releases.iter().chain(&page_cancellations) {
        assert_eq!(key.binding(), BindingId::new(17));
        assert_eq!(key.revision(), SourceRevision::new(2));
        assert!(page_dispatches.contains(key));
        assert_eq!(
            page_releases
                .iter()
                .filter(|settled| *settled == key)
                .count()
                + page_cancellations
                    .iter()
                    .filter(|settled| *settled == key)
                    .count(),
            1
        );
    }
    for key in object_releases.iter().chain(&object_cancellations) {
        assert_eq!(key.binding(), BindingId::new(17));
        assert_eq!(key.revision(), SourceRevision::new(2));
        assert!(object_dispatches.contains(key));
        assert_eq!(
            object_releases
                .iter()
                .filter(|settled| *settled == key)
                .count()
                + object_cancellations
                    .iter()
                    .filter(|settled| *settled == key)
                    .count(),
            1
        );
    }
    for key in &page_dispatches {
        assert_eq!(
            page_releases
                .iter()
                .filter(|settled| *settled == key)
                .count()
                + page_cancellations
                    .iter()
                    .filter(|settled| *settled == key)
                    .count(),
            1
        );
    }
    for key in &object_dispatches {
        assert_eq!(
            object_releases
                .iter()
                .filter(|settled| *settled == key)
                .count()
                + object_cancellations
                    .iter()
                    .filter(|settled| *settled == key)
                    .count(),
            1
        );
    }
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        let ownership = diagnostics.current;
        assert_eq!(
            (
                diagnostics.response_rejection_count,
                diagnostics.last_response_rejection,
                diagnostics.last_response_rejection_stage,
            ),
            rejection_baseline
        );
        assert_eq!(diagnostics.response_rejection_count, 0);
        assert_eq!(diagnostics.last_response_rejection, None);
        assert_eq!(diagnostics.last_response_rejection_stage, None);
        assert!(diagnostics.high_water.owned_bytes <= diagnostics.max_surface_bytes);
        assert!(diagnostics.high_water.owned_items <= diagnostics.max_surface_items);
        assert_eq!(ownership.pending_page_requests, 0);
        assert_eq!(ownership.pending_object_requests, 0);
        assert_eq!(ownership.dispatched_page_requests, 0);
        assert_eq!(ownership.dispatched_object_requests, 0);
        assert_eq!(ownership.active_geometry_jobs, 0);
        assert_eq!(ownership.pending_geometry_pages, 0);
        assert_eq!(ownership.pending_geometry_objects, 0);
        assert_eq!(ownership.pending_target_intents, 0);
        assert_eq!(ownership.pending_index_intents, 0);
        assert_eq!(ownership.pending_layout_intents, 0);
        assert_eq!(ownership.pending_presentation_intents, 0);
        assert_eq!(ownership.pending_rebind_intents, 0);
        assert_eq!(ownership.scheduled_continuations, 0);
        assert_eq!(ownership.queued_requests, 0);
        assert_eq!(ownership.candidates, 0);
        assert_eq!(ownership.page_alias_waits, 0);
        assert_eq!(ownership.resident_geometry_page_waits, 0);
        assert_eq!(ownership.coalesced_geometry_page_waits, 0);
        assert_eq!(ownership.deferred_geometry_responses, 0);
        assert_eq!(ownership.response_custody_count, 0);
        assert_eq!(ownership.response_processing_bytes, 0);
        assert_eq!(ownership.response_processing_items, 0);
        assert_eq!(ownership.deferred_response_bytes, 0);
        assert_eq!(ownership.deferred_response_items, 0);
        assert!(input.is_surface_current_and_interactive());
        let surface = input.surface().unwrap();
        assert_eq!(surface.binding(), binding(&successor, 2));
        assert_eq!(
            surface.composition(),
            Some(ByteRange::from_u64(150, 156).unwrap())
        );
        assert_eq!(
            surface.platform_selection().unwrap(),
            RangeSelection {
                anchor: ByteOffset::new(152),
                head: ByteOffset::new(156),
            }
        );
    });
}

#[gpui::test]
fn mounted_composite_cut_uses_exact_gap_proofs_before_staged_deletion(
    cx: &mut gpui::TestAppContext,
) {
    let source = "ab";
    let (input, cx) = cx
        .add_window_view(|window, cx| RangeTextInput::new(config(source, 1), window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    let object = object_neighbor(91, 10);
    let start = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(object));
    let end = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::after(object));
    let selection = SourceRange::new(start, end).unwrap();
    let facts = [object_fact(91, 1, 10)];
    let (text, objects) = admitted_sources_with_facts(source, 1, &[start, end], &facts);
    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Cut,
                selection,
                MutationPositions::new(end, start, end),
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let RangeTextInputRequest::ObjectPage(request) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("object page")
    };
    let page = restoration_object_page(request, &facts, 980);
    input.update(cx, |input, cx| input.deliver_object_page(page, cx).unwrap());
    let write = (0..3)
        .find_map(
            |_| match input.update(cx, |input, _| input.take_request()) {
                Some(RangeTextInputRequest::ClipboardWrite(write)) => Some(write),
                _ => None,
            },
        )
        .expect("exact value before deletion");
    assert_eq!(write.text(), "[91]");
    input.update(cx, |input, cx| {
        input
            .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx)
            .unwrap();
    });
    let begin = begin_request(&drive_pages(&input, cx, source));
    assert_eq!(begin.proposal().replacement(), selection);
    input.update(cx, |input, cx| {
        input
            .accept_mutation_preflight(begin.proposal().key(), cx)
            .unwrap()
    });
    let staged = drive_pages(&input, cx, source);
    let object_page = staged
        .iter()
        .position(|request| {
            matches!(
                request,
                RangeTextInputRequest::MutationProposalPage(page)
                    if page.page().items().iter().any(|item| matches!(
                        item,
                        gpui_text_input::MutationPageItem::Object(
                            gpui_text_input::ObjectChange::Remove { target }
                        ) if target.range() == selection
                            && target.id() == InlineObjectId::new(91)
                            && target.order() == InlineObjectOrder::new(10)
                    ))
            )
        })
        .expect("exact object removal page");
    let finish = staged
        .iter()
        .position(|request| matches!(request, RangeTextInputRequest::MutationFinishInput(_)))
        .expect("authenticated finish");
    assert!(object_page < finish);
    input.update(cx, |input, cx| {
        input.cancel_mutation(begin.proposal().key(), cx).unwrap();
        assert_eq!(input.surface().unwrap().binding(), binding(source, 1));
    });
}

#[gpui::test]
fn queued_clipboard_write_is_dropped_locally_on_rebind_and_dispose(cx: &mut gpui::TestAppContext) {
    let (rebound, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(config("", 1), window, cx).unwrap());
    assert!(drive_pages(&rebound, cx, "").is_empty());
    let key = queue_empty_clipboard_cut(&rebound, cx);
    cx.update(|window, app| {
        rebound.update(app, |input, cx| {
            input.rebind(binding("", 2), None, window, cx).unwrap()
        })
    });
    let requests = drive_pages(&rebound, cx, "");
    assert!(!requests.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::ClipboardWrite(_)
            | RangeTextInputRequest::CancelClipboardWrite(_)
            | RangeTextInputRequest::MutationBegin(_)
    )));
    rebound.update(cx, |input, cx| {
        assert!(matches!(
            input.settle_clipboard_write(key, ClipboardWriteOutcome::Written, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert!(input.is_quiescent());
    });

    let (disposed, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(config("", 1), window, cx).unwrap());
    assert!(drive_pages(&disposed, cx, "").is_empty());
    let key = queue_empty_clipboard_cut(&disposed, cx);
    let drained =
        cx.update(|window, app| disposed.update(app, |input, cx| input.dispose(window, cx)));
    assert!(!drained.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::ClipboardWrite(_)
            | RangeTextInputRequest::CancelClipboardWrite(_)
            | RangeTextInputRequest::MutationBegin(_)
    )));
    disposed.update(cx, |input, cx| {
        assert!(matches!(
            input.settle_clipboard_write(key, ClipboardWriteOutcome::Written, cx),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
        assert!(input.is_quiescent());
    });
}

#[gpui::test]
fn mounted_read_only_copy_and_single_flight_history_routes_are_typed(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "alpha beta";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    assert!(drive_pages(&input, cx, source).is_empty());
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0, source.len() as u64])
    });

    input.update(cx, |input, cx| input.set_read_only(true, cx));
    cx.simulate_input("blocked");
    cx.simulate_keystrokes("ctrl-a ctrl-x ctrl-z");
    let blocked = drive_pages(&input, cx, source);
    assert!(!blocked.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::MutationBegin(_) | RangeTextInputRequest::HistoryIntent(_)
    )));

    cx.simulate_keystrokes("ctrl-c");
    let copied = drive_pages(&input, cx, source);
    let write = copied
        .iter()
        .find_map(|request| match request {
            RangeTextInputRequest::ClipboardWrite(write) => Some(write),
            _ => None,
        })
        .expect("read-only selection remains copyable");
    assert_eq!(write.text(), source);
    input.update(cx, |input, cx| {
        input
            .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx)
            .unwrap();
        input.set_read_only(false, cx);
    });
    input.update(cx, |input, _| {
        let expected = input.history_frontier();
        input
            .set_history_frontier(
                expected,
                gpui_text_input::RangeHistoryFrontier {
                    binding: binding(source, 1),
                    id: 1,
                    undo_available: true,
                    redo_available: false,
                },
            )
            .unwrap();
    });

    cx.simulate_keystrokes("ctrl-z ctrl-z");
    let history = drive_pages(&input, cx, source);
    let intents = history
        .iter()
        .filter_map(|request| match request {
            RangeTextInputRequest::HistoryIntent(intent) => Some(*intent),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(intents.len(), 1, "history dispatch is single-flight");
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.rebind(binding(source, 2), None, window, cx).unwrap();
        })
    });
    let lifecycle = drive_pages(&input, cx, source);
    assert!(lifecycle.iter().any(|request| matches!(
        request,
        RangeTextInputRequest::CancelHistoryIntent(intent) if *intent == intents[0]
    )));
}

#[gpui::test]
fn saturated_clipboard_text_demand_unwinds_exactly_and_immediate_retry_succeeds(
    cx: &mut gpui::TestAppContext,
) {
    let source = "abc";
    let mut configuration = config(source, 1);
    configuration.residency_limits = ResidencyLimits::new(8, 128 * 1024, 1, 256).unwrap();
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let geometry = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) => Some(page),
            _ => None,
        })
        .expect("initial geometry occupies the pending residency slot");
    let empty_response_custody = input.read_with(cx, |input, _| {
        let current = input.realization_diagnostics().current;
        assert_eq!(current.response_custody_count, 0);
        (
            current.response_custody_bytes,
            current.response_custody_items,
        )
    });
    let start = ordinary_position(0);
    let end = ordinary_position(source.len() as u64);
    let selection = SourceRange::new(start, end).unwrap();
    let (text, objects) = admitted_sources(source, 1, &[start, end]);
    let cut = input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Cut,
                selection,
                MutationPositions::new(end, start, end),
                &text,
                &objects,
                cx,
            )
            .unwrap()
    });
    let object = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::ObjectPage(page) => Some(page),
            _ => None,
        })
        .expect("clipboard object phase");
    let page = restoration_object_page(object, &[], 82_000);
    let published_before = input.read_with(cx, |input, _| {
        input
            .surface()
            .map(|surface| (surface.binding(), surface.selection()))
    });
    input
        .update(cx, |input, cx| input.deliver_object_page(page, cx))
        .unwrap();
    let released = (0..8)
        .filter_map(|_| input.update(cx, |input, _| input.take_request()))
        .collect::<Vec<_>>();
    assert_eq!(released.len(), 1);
    assert!(matches!(
        released.as_slice(),
        [RangeTextInputRequest::ReleaseObjectPage(key)] if *key == object.key()
    ));
    input.read_with(cx, |input, _| {
        let diagnostics = input.realization_diagnostics();
        let current = diagnostics.current;
        assert_eq!(
            input
                .surface()
                .map(|surface| (surface.binding(), surface.selection())),
            published_before
        );
        assert_eq!(current.response_custody_count, 0);
        assert_eq!(current.response_custody_bytes, empty_response_custody.0);
        assert_eq!(current.response_custody_items, empty_response_custody.1);
        assert_eq!(current.response_processing_bytes, 0);
        assert_eq!(current.response_processing_items, 0);
        assert_eq!(current.dispatched_object_requests, 0);
        assert_eq!(current.pending_object_requests, 0);
        assert_eq!(current.clipboard_bytes, 0);
        assert_eq!(current.clipboard_items, 0);
        assert_eq!(current.scheduled_continuations, 0);
        assert_eq!(current.queued_requests, 0);
        assert_eq!(current.pending_page_requests, 1);
        assert_eq!(current.dispatched_page_requests, 1);
        assert_eq!(current.active_geometry_jobs, 1);
        assert!(diagnostics.high_water.owned_bytes <= diagnostics.max_surface_bytes);
        assert!(diagnostics.high_water.owned_items <= diagnostics.max_surface_items);
        assert_eq!(input.clipboard_counts(), Default::default());
    });
    input.update(cx, |input, cx| {
        input
            .fail_page(geometry.key(), PageFailure::Unavailable, cx)
            .unwrap();
        assert!(input.is_quiescent());
    });
    let copy = input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Copy,
                selection,
                MutationPositions::new(end, start, end),
                &text,
                &objects,
                cx,
            )
            .unwrap()
    });
    assert_ne!(copy, cut);
    let object = match input.update(cx, |input, _| input.take_request()) {
        Some(RangeTextInputRequest::ObjectPage(page)) => page,
        request => panic!("released clipboard slot request: {request:?}"),
    };
    let object_key = object.key();
    input.update(cx, |input, cx| {
        input
            .deliver_object_page(restoration_object_page(object, &[], 82_001), cx)
            .unwrap()
    });
    assert!(matches!(
        take_request_after_scheduled_frames(&input, cx, "reused clipboard object release"),
        RangeTextInputRequest::ReleaseObjectPage(key) if key == object_key
    ));
    let RangeTextInputRequest::Page(text_page) =
        take_request_after_scheduled_frames(&input, cx, "reused clipboard text page")
    else {
        panic!("released clipboard slot did not dispatch its text page")
    };
    assert_eq!(text_page.key().purpose(), PagePurpose::Clipboard);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_page(page_for(source, 82_002, text_page), window, cx)
                .unwrap()
        })
    });
    assert!(matches!(
        take_request_after_scheduled_frames(&input, cx, "reused clipboard text release"),
        RangeTextInputRequest::ReleasePage(key) if key == text_page.key()
    ));
    let RangeTextInputRequest::ClipboardWrite(write) =
        take_request_after_scheduled_frames(&input, cx, "reused clipboard write")
    else {
        panic!("reused clipboard text release was not followed by its write")
    };
    assert_eq!(write.text(), source);
    assert_eq!(write.key(), copy);
    input.update(cx, |input, cx| {
        input
            .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx)
            .unwrap();
    });
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    input.read_with(cx, |input, _| assert!(input.is_quiescent()));
}

#[gpui::test]
fn mounted_same_anchor_objects_are_exact_keyboard_steps_and_bounded_metadata(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = vec![
        object_fact(201, 1, 10),
        object_fact(202, 1, 20),
        object_fact_with_activation(203, 1, 30, false),
    ];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    let events = restoration_events(&input, cx);
    let first = object_neighbor(201, 10);
    let middle = object_neighbor(202, 20);
    let last = object_neighbor(203, 30);
    let before = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::before(first));
    let gap_one = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::between(first, middle).unwrap(),
    );
    let gap_two = SourcePosition::new(
        ByteOffset::new(1),
        InlineObjectGap::between(middle, last).unwrap(),
    );
    let after = SourcePosition::new(ByteOffset::new(1), InlineObjectGap::after(last));
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(surface.realized_objects().len(), 3);
        let published = surface
            .realized_presentations(surface.publication_key())
            .unwrap()
            .collect::<Vec<_>>();
        assert_eq!(published.len(), 3);
        assert_eq!(published[0].geometry().leading(), before);
        assert_eq!(published[0].geometry().trailing(), gap_one);
        assert_eq!(published[1].geometry().leading(), gap_one);
        assert_eq!(published[1].geometry().trailing(), gap_two);
        assert_eq!(published[2].geometry().leading(), gap_two);
        assert_eq!(published[2].geometry().trailing(), after);
    });

    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSourceSelection {
                anchor: before,
                head: gap_one,
            }
        );
        assert_eq!(
            input.active_inline_object().unwrap().object_id,
            InlineObjectId::new(201)
        );
    });
    cx.simulate_keystrokes("enter space");
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::InlineObjectActivated(_)))
            .count(),
        2
    );

    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    assert_eq!(
        input.read_with(cx, |input, _| input
            .active_inline_object()
            .unwrap()
            .object_id),
        InlineObjectId::new(202)
    );
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSourceSelection {
                anchor: gap_two,
                head: after,
            }
        );
        assert_eq!(
            input.active_inline_object().unwrap().object_id,
            InlineObjectId::new(203)
        );
    });
    for key in ["enter", "space"] {
        cx.simulate_keystrokes(key);
        let RangeTextInputRequest::MutationBegin(begin) =
            input.update(cx, |input, _| input.take_request()).unwrap()
        else {
            panic!("ineligible activation retains ordinary key behavior")
        };
        input.update(cx, |input, cx| {
            input
                .reject_mutation_preflight(begin.proposal().key(), cx)
                .unwrap();
        });
    }
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| matches!(event, RangeTextInputEvent::InlineObjectActivated(_)))
            .count(),
        2
    );
}

#[gpui::test]
fn one_object_backspace_delete_replacement_and_read_only_use_exact_remove(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    let source = "ab";
    let facts = vec![object_fact(401, 1, 10)];
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
        input.focus(window);
        input
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("right");
    drive_pages_with_objects(&input, cx, source, &facts);
    let object = input.read_with(cx, |input, _| {
        input.surface().unwrap().realized_objects()[0]
    });
    let exact = SourceRange::new(object.leading(), object.trailing()).unwrap();

    for replacement in [None, Some("X")] {
        match replacement {
            None => cx.simulate_keystrokes("backspace"),
            Some(text) => cx.simulate_input(text),
        }
        let RangeTextInputRequest::MutationBegin(begin) =
            input.update(cx, |input, _| input.take_request()).unwrap()
        else {
            panic!("exact object mutation begin")
        };
        assert_eq!(begin.proposal().replacement(), exact);
        input.update(cx, |input, cx| {
            input
                .accept_mutation_preflight(begin.proposal().key(), cx)
                .unwrap()
        });
        let staged = drive_pages(&input, cx, source);
        let page = staged
            .iter()
            .find_map(|request| match request {
                RangeTextInputRequest::MutationProposalPage(page) => Some(page.page()),
                _ => None,
            })
            .expect("proposal page");
        assert!(page.items().iter().any(|item| matches!(
            item,
            gpui_text_input::MutationPageItem::Object(
                gpui_text_input::ObjectChange::Remove { target }
            ) if target.range() == exact
                && target.id() == InlineObjectId::new(401)
                && target.order() == InlineObjectOrder::new(10)
        )));
        assert_eq!(
            page.items()
                .iter()
                .filter(|item| matches!(item, gpui_text_input::MutationPageItem::Utf8 { .. }))
                .count(),
            usize::from(replacement.is_some())
        );
        input.update(cx, |input, cx| {
            input.cancel_mutation(begin.proposal().key(), cx).unwrap();
        });
        drive_pages(&input, cx, source);
        input.read_with(cx, |input, _| {
            assert_eq!(
                input.surface().unwrap().selection(),
                RangeSourceSelection {
                    anchor: object.leading(),
                    head: object.trailing(),
                }
            );
        });
    }

    let before_click = input.read_with(cx, |input, _| {
        input
            .surface()
            .unwrap()
            .realized_object_gaps()
            .iter()
            .find(|gap| gap.position() == object.leading())
            .unwrap()
            .caret_bounds()
            .origin
    });
    cx.simulate_event(MouseDownEvent {
        position: before_click,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: 1,
        first_mouse: false,
    });
    drive_pages_with_objects(&input, cx, source, &facts);
    cx.simulate_keystrokes("delete");
    let RangeTextInputRequest::MutationBegin(delete) =
        input.update(cx, |input, _| input.take_request()).unwrap()
    else {
        panic!("delete begin")
    };
    assert_eq!(delete.proposal().replacement(), exact);
    input.update(cx, |input, cx| {
        input
            .reject_mutation_preflight(delete.proposal().key(), cx)
            .unwrap();
        input.set_read_only(true, cx);
    });
    cx.simulate_keystrokes("delete backspace");
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
}

#[gpui::test]
fn committed_object_removal_and_replacement_emit_exact_realization_loss(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(ensure_text_input_bindings);
    for (case, replacement) in [(0u128, None), (1, Some("X"))] {
        let source = "ab";
        let object_id = 700 + case;
        let facts = [object_fact(object_id, 1, 10)];
        let (input, cx) = cx.add_window_view(|window, cx| {
            let input = RangeTextInput::new(config(source, 1), window, cx).unwrap();
            input.focus(window);
            input
        });
        drive_pages_with_objects(&input, cx, source, &facts);
        let events = restoration_events(&input, cx);
        let object = input.read_with(cx, |input, _| {
            input.surface().unwrap().realized_objects()[0]
        });
        cx.simulate_event(MouseDownEvent {
            position: object.hit_bounds().origin + gpui::point(px(1.), px(1.)),
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        drive_pages_with_objects(&input, cx, source, &facts);
        let active = input.read_with(cx, |input, _| input.active_inline_object().unwrap());
        let attached = input
            .update(cx, |input, _| {
                input.attach_active_inline_object_surface(active)
            })
            .unwrap();

        match replacement {
            None => cx.simulate_keystrokes("backspace"),
            Some(text) => cx.simulate_input(text),
        }
        let RangeTextInputRequest::MutationBegin(begin) =
            input.update(cx, |input, _| input.take_request()).unwrap()
        else {
            panic!("object commit begin")
        };
        let finish = accept_local_to_commit(&input, cx, source, begin);
        let positions = finish.intended();
        let successor = replacement.map_or_else(|| source.to_owned(), |text| format!("a{text}b"));
        let successor_positions = [
            positions.caret(),
            positions.selection_anchor(),
            positions.selection_head(),
        ];
        let (text, objects) = admitted_sources_with_facts(&successor, 2, &successor_positions, &[]);
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                input
                    .settle_committed_mutation(
                        begin.proposal().key(),
                        binding(&successor, 2),
                        positions,
                        &text,
                        &objects,
                        window,
                        cx,
                    )
                    .unwrap();
            })
        });
        drive_pages_with_objects(&input, cx, &successor, &[]);
        assert!(input.read_with(cx, |input, _| input.active_inline_object().is_none()));
        assert_eq!(
            events
                .borrow()
                .iter()
                .filter(|event| matches!(
                    event,
                    RangeTextInputEvent::InlineObjectRealizationLost(loss)
                        if loss.anchor == active
                            && loss.reason
                                == gpui_text_input::InlineObjectRealizationLossReason::Removed
                ))
                .count(),
            1
        );
        assert!(matches!(
            cx.update(|window, app| input.update(app, |input, cx| input
                .dismiss_active_inline_object_surface(
                    attached,
                    InlineObjectSurfaceDismissal::RefocusObject,
                    window,
                    cx,
                ))),
            Err(gpui_text_input::RangeTextInputError::Stale)
        ));
    }
}
