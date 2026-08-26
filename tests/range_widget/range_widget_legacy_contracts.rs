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
    let source = "abcdefghij".repeat(20);
    let inserted = "\u{00e9}\u{1f642}";
    let (input, cx) = cx.add_window_view(|window, cx| {
        let input = RangeTextInput::new(config(&source, 1), window, cx).unwrap();
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
    cx.update(|window, app| {
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
                .unwrap();
        })
    });
    assert!(drive_pages(&input, cx, &successor).is_empty());
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
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
    let start = ordinary_position(0);
    let end = ordinary_position(source.len() as u64);
    let selection = SourceRange::new(start, end).unwrap();
    let (text, objects) = admitted_sources(source, 1, &[start, end]);
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
    let object = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::ObjectPage(page) => Some(page),
            _ => None,
        })
        .expect("clipboard object phase");
    let page = restoration_object_page(object, &[], 82_000);
    assert!(matches!(
        input.update(cx, |input, cx| input.deliver_object_page(page, cx)),
        Err(gpui_text_input::RangeTextInputError::Busy)
    ));
    let failed = (0..8)
        .filter_map(|_| input.update(cx, |input, _| input.take_request()))
        .collect::<Vec<_>>();
    assert!(
        failed
            .iter()
            .all(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(_)))
    );
    input.update(cx, |input, cx| {
        input
            .fail_page(geometry.key(), PageFailure::Unavailable, cx)
            .unwrap();
        assert!(input.is_quiescent());
        input
            .begin_composite_clipboard(
                gpui_text_input::ClipboardKind::Copy,
                selection,
                MutationPositions::new(end, start, end),
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let object = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::ObjectPage(page) => Some(page),
            _ => None,
        })
        .expect("immediate retry object page");
    input.update(cx, |input, cx| {
        input
            .deliver_object_page(restoration_object_page(object, &[], 82_001), cx)
            .unwrap()
    });
    let text_page = input
        .update(cx, |input, _| input.take_request())
        .and_then(|request| match request {
            RangeTextInputRequest::Page(page) if page.key().purpose() == PagePurpose::Clipboard => {
                Some(page)
            }
            _ => None,
        })
        .expect("retry owns released text slot");
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_page(page_for(source, 82_002, text_page), window, cx)
                .unwrap()
        })
    });
    let write = (0..4)
        .find_map(
            |_| match input.update(cx, |input, _| input.take_request()) {
                Some(RangeTextInputRequest::ClipboardWrite(write)) => Some(write),
                _ => None,
            },
        )
        .expect("retry clipboard write");
    assert_eq!(write.text(), source);
    input.update(cx, |input, cx| {
        input
            .settle_clipboard_write(write.key(), ClipboardWriteOutcome::Written, cx)
            .unwrap();
    });
    while input.update(cx, |input, _| input.take_request()).is_some() {}
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
