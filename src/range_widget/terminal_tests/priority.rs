use super::*;

#[gpui::test]
fn offscreen_caret_ime_and_directed_selection_drive_realized_priority(
    cx: &mut gpui::TestAppContext,
) {
    let source = (0..160)
        .map(|line| format!("priority-{line:03}\n"))
        .collect::<String>();
    let end = source.len() as u64;
    let tail_offset = end / 2;
    for expected in [
        RangeRealizationPriority::Caret,
        RangeRealizationPriority::Ime,
        RangeRealizationPriority::DirectedSelection,
    ] {
        let mut configuration = config(2 * 1024 * 1024, 32_768);
        configuration.binding = RangeBinding::new(
            BindingId::new(84 + expected as u64),
            SourceRevision::new(1),
            LogicalExtent::new(end, 160),
        );
        configuration.viewport_extent = px(80.);
        configuration.limits.max_realized_block_extent = px(32.);
        let (input, cx) = cx.add_window_view(move |window, cx| {
            RangeTextInput::new(configuration, window, cx).unwrap()
        });
        drive_surface_for_source(&input, cx, &source);
        input.read_with(cx, |input, _| {
            let tail = SourcePosition::new(
                ByteOffset::new(tail_offset),
                crate::InlineObjectGap::no_objects(),
            );
            assert!(
                input
                    .surface()
                    .unwrap()
                    .position_for_source_position(tail)
                    .is_none()
            );
        });
        input.update(cx, |input, cx| {
            let tail = SourcePosition::new(
                ByteOffset::new(tail_offset),
                crate::InlineObjectGap::no_objects(),
            );
            let prior = SourcePosition::new(
                ByteOffset::new(tail_offset - 1),
                crate::InlineObjectGap::no_objects(),
            );
            let origin =
                SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::no_objects());
            let mut desired = input.desired;
            desired.reveal_caret = true;
            desired.source_selection = Some(match expected {
                RangeRealizationPriority::Caret => RangeSourceSelection::caret(tail),
                RangeRealizationPriority::Ime => RangeSourceSelection {
                    anchor: prior,
                    head: tail,
                },
                RangeRealizationPriority::DirectedSelection => RangeSourceSelection {
                    anchor: origin,
                    head: tail,
                },
                _ => unreachable!(),
            });
            desired.composition = (expected == RangeRealizationPriority::Ime)
                .then(|| ByteRange::from_u64(tail_offset - 1, tail_offset).unwrap());
            let candidate = input.prepare_target_transition(desired, None).unwrap();
            input.commit_widget_transition(candidate, Some(cx));
        });
        drive_surface_for_source(&input, cx, &source);
        input.read_with(cx, |input, _| {
            let surface = input.surface().unwrap();
            assert_eq!(surface.realization_priority(), expected);
            assert_eq!(
                surface.selection().head.byte_offset,
                ByteOffset::new(tail_offset)
            );
            assert!(
                surface
                    .position_for_source_position(surface.selection().head)
                    .is_some()
            );
            assert_eq!(
                surface.composition().is_some(),
                expected == RangeRealizationPriority::Ime
            );
            assert_eq!(surface.filler_count(), 1);
            assert_eq!(
                surface.capacity_state(),
                RangeRealizationCapacityState::ViewportExceedsRenderingCapacity
            );
        });
        input.update(cx, |input, cx| {
            input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
            let target = input.surface().unwrap().scroll_block();
            input.request_absolute_scroll(target, cx).unwrap();
        });
        drive_surface_for_source(&input, cx, &source);
        input.read_with(cx, |input, _| {
            assert_eq!(
                input.surface().unwrap().capacity_state(),
                RangeRealizationCapacityState::ViewportExceedsRenderingCapacity
            );
        });
    }
}

#[gpui::test]
fn active_interaction_and_scroll_anchor_are_runtime_realization_targets(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    let object =
        crate::InlineObjectNeighbor::new(InlineObjectId::new(51), InlineObjectOrder::new(7));
    let fact = InlineObjectFact::new(
        object.id(),
        ByteOffset::new(4),
        object.order(),
        "resident-object-display",
        InlineObjectPresentation::new(
            51,
            SharedString::new_static(""),
            px(18.),
            px(16.),
            px(10.),
            None,
            0,
            true,
        )
        .unwrap(),
    );
    input.update(cx, |input, _| {
        let mut desired = input.desired;
        desired.source_selection = Some(RangeSourceSelection {
            anchor: SourcePosition::new(ByteOffset::new(4), crate::InlineObjectGap::before(object)),
            head: SourcePosition::new(ByteOffset::new(4), crate::InlineObjectGap::after(object)),
        });
        desired.reveal_caret = false;
        desired.preserve_scroll_anchor = false;
        desired.inline_object_interaction = Some(DesiredInlineObjectInteraction::Set {
            object_id: object.id(),
            order: object.order(),
            activation_eligible: true,
            origin: None,
        });
        let candidate = input.prepare_target_transition(desired, None).unwrap();
        input.commit_widget_transition(candidate, None);
    });
    let mut next_page = 100_000;
    drive_bounded_priority_objects(&input, cx, SOURCE, &[fact], &mut next_page);
    input.read_with(cx, |input, _| {
        let surface = input.surface().unwrap();
        assert_eq!(
            surface.realization_priority(),
            RangeRealizationPriority::ActiveInteraction
        );
        assert!(
            surface
                .realized_objects()
                .iter()
                .any(|realized| realized.id() == object.id())
        );
    });
    input.update(cx, |input, cx| {
        let block = input.surface().unwrap().scroll_block();
        input.request_absolute_scroll(block, cx).unwrap();
    });
    drive_surface_for_source(&input, cx, SOURCE);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().realization_priority(),
            RangeRealizationPriority::ScrollAnchor
        );
    });
}

#[gpui::test]
fn equal_stable_ids_count_distinct_surface_and_residency_allocations_exactly(
    cx: &mut gpui::TestAppContext,
) {
    let fact = InlineObjectFact::new(
        InlineObjectId::new(51),
        ByteOffset::new(4),
        InlineObjectOrder::new(7),
        "resident-object-display",
        InlineObjectPresentation::new(
            51,
            SharedString::new_static(""),
            px(18.),
            px(16.),
            px(10.),
            None,
            0,
            true,
        )
        .unwrap(),
    );
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    input.update(cx, |input, cx| {
        let neighbor = crate::InlineObjectNeighbor::new(fact.id(), fact.order());
        let mut desired = input.desired;
        desired.source_selection = Some(RangeSourceSelection {
            anchor: SourcePosition::new(
                ByteOffset::new(4),
                crate::InlineObjectGap::before(neighbor),
            ),
            head: SourcePosition::new(ByteOffset::new(4), crate::InlineObjectGap::after(neighbor)),
        });
        desired.reveal_caret = false;
        desired.preserve_scroll_anchor = false;
        desired.inline_object_interaction = Some(DesiredInlineObjectInteraction::Set {
            object_id: fact.id(),
            order: fact.order(),
            activation_eligible: true,
            origin: None,
        });
        input.desired = desired;
        cx.notify();
    });
    let mut next_page = 210_000;
    drive_bounded_priority_objects(
        &input,
        cx,
        SOURCE,
        std::slice::from_ref(&fact),
        &mut next_page,
    );

    input.update(cx, |input, entity_cx| {
        let surface_text = input.surface().unwrap().pages()[0].clone();
        let surface_object = input
            .surface()
            .unwrap()
            .object_pages()
            .iter()
            .find(|page| !page.objects().is_empty())
            .unwrap()
            .clone();
        let resident_text_ids = input
            .residency
            .resident_pages()
            .map(RangePage::id)
            .collect::<Vec<_>>();
        for id in resident_text_ids {
            assert!(input.residency.evict(id));
        }
        let resident_object_ids = input
            .object_residency
            .resident_pages()
            .map(ObjectPage::id)
            .collect::<Vec<_>>();
        for id in resident_object_ids {
            assert!(input.object_residency.evict(id));
        }

        let PageDemand::Requested(text_request) = input
            .residency
            .demand(
                PageRequestId::new(900_000),
                surface_text.key().purpose(),
                surface_text.key().demand(),
            )
            .unwrap()
        else {
            panic!("fresh equal-id text demand")
        };
        let duplicate_text = surface_text.clone_for_request(text_request.key()).unwrap();
        assert_ne!(surface_text.text().as_ptr(), duplicate_text.text().as_ptr());
        input.residency.admit(duplicate_text).unwrap();

        let ObjectDemand::Requested(object_request) = input
            .object_residency
            .demand(
                ObjectRequestId::new(900_001),
                surface_object.key().purpose(),
                surface_object.key().demand(),
            )
            .unwrap()
        else {
            panic!("fresh equal-id object demand")
        };
        let duplicate_object = ObjectPage::new(
            surface_object.id(),
            object_request.key(),
            surface_object.objects().to_vec(),
            surface_object.preceding(),
            surface_object.following(),
            surface_object.complete(),
            surface_object.continuation(),
        )
        .unwrap();
        assert_ne!(
            surface_object.objects().as_ptr(),
            duplicate_object.objects().as_ptr()
        );
        let proofs = input
            .residency
            .prove_object_page_anchors(input.config.binding, &duplicate_object)
            .unwrap();
        input
            .object_residency
            .admit(duplicate_object, proofs)
            .unwrap();
        input.observe_realization_ownership();

        let surface = input.surface().unwrap();
        let expected_text_bytes = surface
            .pages()
            .iter()
            .chain(input.residency.resident_pages())
            .map(|page| page.retained_charge().bytes())
            .sum::<usize>();
        let expected_object_bytes = surface
            .object_pages()
            .iter()
            .chain(input.object_residency.resident_pages())
            .map(|page| page.retained_charge().bytes())
            .sum::<usize>();
        let diagnostics = input.realization_diagnostics();
        assert_eq!(diagnostics.current.resident_page_bytes, expected_text_bytes);
        assert_eq!(
            diagnostics.current.resident_object_bytes,
            expected_object_bytes
        );
        assert_eq!(
            diagnostics.current.resident_pages,
            surface.pages().len() + input.residency.resident_pages().count()
        );
        assert_eq!(
            diagnostics.current.resident_objects,
            surface
                .object_pages()
                .iter()
                .map(|page| page.objects().len())
                .sum::<usize>()
                + input
                    .object_residency
                    .resident_pages()
                    .map(|page| page.objects().len())
                    .sum::<usize>()
        );
        assert!(diagnostics.high_water.resident_page_bytes >= expected_text_bytes);
        assert!(diagnostics.high_water.resident_object_bytes >= expected_object_bytes);

        let current = diagnostics.current;
        let candidate_charge = RangeSurfaceCharge {
            bytes: std::mem::size_of::<super::super::transition::ActiveObjectTransitionCandidate>(),
            items: 1,
        };
        input.config.limits.max_surface_bytes = current.owned_bytes + candidate_charge.bytes - 1;
        input.config.limits.max_surface_items = current.owned_items + candidate_charge.items;
        assert!(matches!(
            input.prepare_interaction_state_transition(
                !input.enabled,
                input.pointer_anchor,
                super::super::transition::ActiveObjectTransition::Preserve,
            ),
            Err(RangeTextInputError::SurfaceCapacity)
        ));
        input.config.limits.max_surface_bytes = current.owned_bytes + candidate_charge.bytes;
        let candidate = input
            .prepare_interaction_state_transition(
                !input.enabled,
                input.pointer_anchor,
                super::super::transition::ActiveObjectTransition::Preserve,
            )
            .unwrap();
        input.commit_active_object_transition(candidate, entity_cx);
        assert!(
            input.realization_diagnostics().surface_high_water.bytes
                >= current.owned_bytes + candidate_charge.bytes
        );
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
            let _ = input.dispose(window, cx);
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.current.resident_page_bytes, 0);
            assert_eq!(diagnostics.current.resident_object_bytes, 0);
        })
    });
}

fn bounded_priority_object_page(
    request: crate::ObjectRequest,
    facts: &[InlineObjectFact],
    id: u64,
) -> ObjectPage {
    let demand = request.key().demand();
    let eligible = facts
        .iter()
        .filter(|fact| demand.contains_anchor(fact.anchor()))
        .filter(|fact| demand.cursor().is_none_or(|cursor| fact.cursor() > cursor))
        .take(8)
        .cloned()
        .collect::<Vec<_>>();
    let remaining = facts.iter().any(|fact| {
        demand.contains_anchor(fact.anchor())
            && demand.cursor().is_none_or(|cursor| fact.cursor() > cursor)
            && eligible
                .last()
                .is_some_and(|last| fact.cursor() > last.cursor())
    });
    let continuation = remaining.then(|| eligible.last().unwrap().cursor());
    ObjectPage::new(
        ObjectPageId::new(id),
        request.key(),
        eligible,
        demand.cursor().map_or(
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::Continues,
        ),
        continuation.map_or(
            ObjectPageEdgeFact::EnvelopeBoundary,
            ObjectPageEdgeFact::Continues,
        ),
        !remaining,
        continuation,
    )
    .unwrap()
}

fn drive_bounded_priority_objects(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
    facts: &[InlineObjectFact],
    next_page: &mut u64,
) {
    for _ in 0..512 {
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.begin_realization_frame();
                input
                    .service_admitted_geometry_for_prepaint(window, cx)
                    .unwrap();
            })
        });
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for_source(request, *next_page, source);
                *next_page += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = bounded_priority_object_page(request, facts, *next_page);
                *next_page += 1;
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap_or_else(|error| {
                                panic!(
                                    "priority object delivery failed for {:?} from {request:?}: {error:?}",
                                    input.target_intent_desired().source_selection,
                                )
                            })
                    })
                });
            }
            Some(
                RangeTextInputRequest::ReleasePage(_)
                | RangeTextInputRequest::ReleaseObjectPage(_)
                | RangeTextInputRequest::CancelPage(_)
                | RangeTextInputRequest::CancelObjectPage(_),
            ) => {}
            Some(request) => panic!("unexpected priority request: {request:?}"),
            None if input.read_with(cx, |input, _| input.is_quiescent()) => return,
            None => {}
        }
    }
    panic!("bounded priority object drive did not quiesce");
}

#[gpui::test]
fn exact_priority_gap_crosses_more_same_anchor_objects_than_residency_capacity(
    cx: &mut gpui::TestAppContext,
) {
    let source = "ab";
    let facts = (0..96)
        .map(|index| {
            InlineObjectFact::new(
                InlineObjectId::new(10_000 + index),
                ByteOffset::new(1),
                InlineObjectOrder::new(index + 1),
                format!("[{index}]"),
                InlineObjectPresentation::new(
                    index as u64,
                    SharedString::new_static("x"),
                    px(100.),
                    px(24.),
                    px(20.),
                    None,
                    0,
                    true,
                )
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let neighbor =
        |index: usize| crate::InlineObjectNeighbor::new(facts[index].id(), facts[index].order());
    let gaps = [
        SourcePosition::new(
            ByteOffset::new(1),
            crate::InlineObjectGap::before(neighbor(0)),
        ),
        SourcePosition::new(
            ByteOffset::new(1),
            crate::InlineObjectGap::between(neighbor(1), neighbor(2)).unwrap(),
        ),
        SourcePosition::new(
            ByteOffset::new(1),
            crate::InlineObjectGap::between(neighbor(47), neighbor(48)).unwrap(),
        ),
        SourcePosition::new(
            ByteOffset::new(1),
            crate::InlineObjectGap::between(neighbor(80), neighbor(81)).unwrap(),
        ),
        SourcePosition::new(
            ByteOffset::new(1),
            crate::InlineObjectGap::after(neighbor(95)),
        ),
    ];
    for (gap_index, gap) in gaps.into_iter().enumerate() {
        for expected in [
            RangeRealizationPriority::Caret,
            RangeRealizationPriority::Ime,
            RangeRealizationPriority::DirectedSelection,
        ] {
            let mut configuration = config(4 * 1024 * 1024, 65_536);
            configuration.binding = RangeBinding::new(
                BindingId::new(110 + gap_index as u64 * 8 + expected as u64),
                SourceRevision::new(1),
                LogicalExtent::new(source.len() as u64, 1),
            );
            configuration.geometry_limits =
                ExactGeometryLimits::new(32, 128, 512 * 1024, 8192).unwrap();
            configuration.limits.max_realized_block_extent = px(24.);
            configuration.overscan = Pixels::ZERO;
            configuration.object_residency_limits =
                ObjectResidencyLimits::new(2, 16, 256 * 1024, 64 * 1024, 2, 16, 256 * 1024)
                    .unwrap();
            let (input, cx) = cx.add_window_view(move |window, cx| {
                RangeTextInput::new(configuration, window, cx).unwrap()
            });
            let mut next_page = 120_000;
            input.update(cx, |input, _| {
                let origin =
                    SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::no_objects());
                let prior = origin;
                let mut desired = input.desired;
                desired.reveal_caret = true;
                desired.preserve_scroll_anchor = false;
                desired.source_selection = Some(match expected {
                    RangeRealizationPriority::Caret => RangeSourceSelection::caret(gap),
                    RangeRealizationPriority::Ime => RangeSourceSelection {
                        anchor: prior,
                        head: gap,
                    },
                    RangeRealizationPriority::DirectedSelection => RangeSourceSelection {
                        anchor: origin,
                        head: gap,
                    },
                    _ => unreachable!(),
                });
                desired.composition = (expected == RangeRealizationPriority::Ime)
                    .then(|| ByteRange::from_u64(0, 1).unwrap());
                let candidate = input.prepare_target_transition(desired, None).unwrap();
                input.commit_widget_transition(candidate, None);
            });
            drive_bounded_priority_objects(&input, cx, source, &facts, &mut next_page);
            input.read_with(cx, |input, _| {
                let surface = input.surface().unwrap();
                assert_eq!(surface.realization_priority(), expected);
                assert_eq!(surface.selection().head, gap);
                assert!(
                    surface.position_for_source_position(gap).is_some(),
                    "missing gap {gap:?} for case {gap_index} at priority {expected:?}; realized gaps: {:?}",
                    surface.realized_object_gaps(),
                );
                assert!(
                    surface
                        .realized_object_gaps()
                        .iter()
                        .any(|realized| realized.position() == gap)
                );
                let realized_presentations = surface
                    .realized_presentations(surface.publication_key())
                    .unwrap()
                    .collect::<Vec<_>>();
                assert_eq!(
                    realized_presentations.len(),
                    surface.realized_objects().len()
                );
                if gap_index == 2 {
                    assert!(!realized_presentations.is_empty());
                }
                for realized in realized_presentations {
                    let expected = facts
                        .iter()
                        .find(|fact| fact.id() == realized.geometry().id())
                        .unwrap();
                    assert_eq!(realized.presentation(), expected.presentation());
                }
                let diagnostics = input.realization_diagnostics();
                assert!(diagnostics.current.resident_objects <= diagnostics.max_owned_objects);
                assert!(diagnostics.current.resident_pages <= diagnostics.max_owned_pages);
                assert!(diagnostics.current.pending_object_requests <= 2);
            });
            cx.update(|window, app| {
                input.update(app, |input, cx| {
                    let _ = input.dispose(window, cx);
                    let diagnostics = input.realization_diagnostics();
                    assert_eq!(diagnostics.current.resident_pages, 0);
                    assert_eq!(diagnostics.current.resident_objects, 0);
                    assert_eq!(diagnostics.current.pending_object_requests, 0);
                })
            });
        }
    }
}

#[gpui::test]
fn exact_priority_after_end_object_retains_proof_for_successive_edit(
    cx: &mut gpui::TestAppContext,
) {
    let source = "a";
    let first = InlineObjectFact::new(
        InlineObjectId::new(20_000),
        ByteOffset::new(source.len() as u64),
        InlineObjectOrder::new(1),
        "[first]",
        InlineObjectPresentation::new(
            20_000,
            SharedString::new_static("x"),
            px(16.),
            px(24.),
            px(20.),
            None,
            0,
            true,
        )
        .unwrap(),
    );
    let gap = SourcePosition::new(
        ByteOffset::new(source.len() as u64),
        crate::InlineObjectGap::after(crate::InlineObjectNeighbor::new(first.id(), first.order())),
    );
    let facts = [first];
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(190),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 1),
    );
    configuration.geometry_limits = ExactGeometryLimits::new(32, 4, 512 * 1024, 8192).unwrap();
    configuration.object_residency_limits =
        ObjectResidencyLimits::new(2, 1, 64 * 1024, 64 * 1024, 2, 1, 64 * 1024).unwrap();
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let mut next_page = 190_000;
    drive_bounded_priority_objects(&input, cx, source, &facts, &mut next_page);
    input.update(cx, |input, cx| {
        input
            .publish_source_selection(RangeSourceSelection::caret(gap), None, None, cx)
            .unwrap();
    });
    drive_bounded_priority_objects(&input, cx, source, &facts, &mut next_page);
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().selection(),
            RangeSourceSelection::caret(gap)
        );
        assert_eq!(input.realization_diagnostics().current.resident_objects, 1);
    });
    input.update(cx, |input, cx| {
        input
            .insert_inline_object_at_selection(
                InlineObjectId::new(20_001),
                InlineObjectOrder::new(2),
                1,
                0,
                cx,
            )
            .unwrap();
    });
}
