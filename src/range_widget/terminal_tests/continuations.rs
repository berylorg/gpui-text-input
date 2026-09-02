use super::*;

const PAGE_BYTES: u64 = 8;
const PAGE_COUNT: usize = 32;

fn continuation_source() -> String {
    "a".repeat(PAGE_BYTES as usize * PAGE_COUNT)
}

fn continuation_config(source: &str) -> RangeTextInputConfig {
    let mut configuration = config(8 * 1024 * 1024, 65_536);
    configuration.binding = RangeBinding::new(
        BindingId::new(71),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 1),
    );
    configuration.residency_limits =
        ResidencyLimits::new(PAGE_COUNT + 4, 256 * 1024, 8, 512).unwrap();
    configuration.limits.max_realization_work_per_frame = 1;
    configuration.limits.page_bytes = PAGE_BYTES;
    configuration.limits.platform_bytes = source.len() as u64;
    configuration.segmentation_limits = SegmentationLimits::new(PAGE_BYTES, 64).unwrap();
    configuration
}

fn seed_resident_chain(input: &mut RangeTextInput, source: &str) {
    let mut residency = RangeResidency::new(input.config.binding, input.config.residency_limits);
    for page_index in 0..PAGE_COUNT {
        let start = page_index as u64 * PAGE_BYTES;
        let end = start + PAGE_BYTES;
        let id = PageRequestId::new(300_000 + page_index as u64);
        let demand = PageDemandEnvelope::Adjacent {
            anchor: ByteOffset::new(start),
            direction: PageDirection::Forward,
            max_payload_bytes: PAGE_BYTES,
        };
        let PageDemand::Requested(request) =
            residency.demand(id, PagePurpose::Caret, demand).unwrap()
        else {
            panic!("fresh chain page must request exact host payload")
        };
        let page = RangePage::new(
            PageId::new(300_000 + page_index as u64),
            request.key(),
            ByteRange::from_u64(start, end).unwrap(),
            source[start as usize..end as usize].to_owned(),
            vec![],
            if start == 0 {
                PageEdgeFact::DocumentBoundary
            } else {
                PageEdgeFact::Continues
            },
            if end == source.len() as u64 {
                PageEdgeFact::DocumentBoundary
            } else {
                PageEdgeFact::Continues
            },
            end == source.len() as u64,
        )
        .unwrap();
        residency.admit(page).unwrap();
    }
    assert_eq!(residency.counts().resident_pages, PAGE_COUNT);
    input.residency = residency;
    input.observe_realization_ownership();
}

fn new_resident_chain(
    cx: &mut gpui::TestAppContext,
) -> (
    String,
    gpui::Entity<RangeTextInput>,
    &mut gpui::VisualTestContext,
) {
    let source = continuation_source();
    let configuration = continuation_config(&source);
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);
    input.update(cx, |input, _| seed_resident_chain(input, &source));
    (source, input, cx)
}

fn service_one_resident_advance(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) {
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert!(!input.response_custody.is_empty());
            input.begin_realization_frame();
            assert!(custody_progressed(
                input.service_response_custody(window, cx)
            ));
            let diagnostics = input.realization_diagnostics();
            assert_eq!(diagnostics.frame.spent, 1);
            assert_eq!(diagnostics.frame.remaining, 0);
            assert!(input.response_custody.len() <= 1);
        })
    });
}

fn drain_resident_advances(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
) -> usize {
    let mut advances = 0;
    while input.read_with(cx, |input, _| !input.response_custody.is_empty()) {
        assert!(
            advances < PAGE_COUNT + 4,
            "resident continuation did not terminate"
        );
        service_one_resident_advance(input, cx);
        advances += 1;
    }
    advances
}

fn assert_many_bounded_advances(advances: usize) {
    assert!(advances >= PAGE_COUNT / 4);
    assert!(advances <= PAGE_COUNT);
}

fn dispose_resident_chain(input: &gpui::Entity<RangeTextInput>, cx: &mut gpui::VisualTestContext) {
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
            let current = input.realization_diagnostics().current;
            assert_eq!(current.response_custody_count, 0);
            assert_eq!(current.response_custody_bytes, 0);
            assert_eq!(current.response_processing_bytes, 0);
        })
    });
    cx.run_until_parked();
}

#[gpui::test]
fn resident_platform_replay_advances_one_page_per_frame(cx: &mut gpui::TestAppContext) {
    let (source, input, cx) = new_resident_chain(cx);
    assert!(matches!(
        input.update(cx, |input, cx| input
            .platform_text_for_range(0..source.len(), cx)
            .unwrap()),
        crate::PlatformRangeResult::Pending(_)
    ));
    service_one_resident_advance(&input, cx);
    input.read_with(cx, |input, _| {
        assert!(input.platform.is_some());
        assert_eq!(input.response_custody.len(), 1);
    });
    let later_advances = drain_resident_advances(&input, cx);
    assert_many_bounded_advances(later_advances + 1);
    assert_eq!(
        input.update(cx, |input, cx| input
            .platform_text_for_range(0..source.len(), cx)
            .unwrap()),
        crate::PlatformRangeResult::Ready(source)
    );
    dispose_resident_chain(&input, cx);
}

#[gpui::test]
fn resident_segmentation_advances_one_page_per_frame(cx: &mut gpui::TestAppContext) {
    let (source, input, cx) = new_resident_chain(cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .begin_boundary_from(
                    ByteOffset::new(0),
                    crate::SegmentationKind::LogicalLine,
                    crate::SegmentationDirection::Forward,
                    super::super::interaction::PendingBoundaryAction::Move {
                        extend: false,
                        direction: crate::SegmentationDirection::Forward,
                    },
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    service_one_resident_advance(&input, cx);
    input.read_with(cx, |input, _| {
        assert!(input.segmentation.is_some());
        assert_eq!(input.response_custody.len(), 1);
    });
    let later_advances = drain_resident_advances(&input, cx);
    assert_many_bounded_advances(later_advances + 1);
    input.read_with(cx, |input, _| {
        assert!(input.segmentation.is_none());
        assert_eq!(
            input
                .target_intent_desired()
                .source_selection
                .unwrap()
                .head
                .byte_offset,
            ByteOffset::new(source.len() as u64)
        );
    });
    dispose_resident_chain(&input, cx);
}

#[gpui::test]
fn resident_replacement_scan_advances_one_page_per_frame(cx: &mut gpui::TestAppContext) {
    let (source, input, cx) = new_resident_chain(cx);
    let start = SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::no_objects());
    let end = SourcePosition::new(
        ByteOffset::new(source.len() as u64),
        crate::InlineObjectGap::no_objects(),
    );
    let (_, text, objects) = admitted_successor_sources(&source, 1, &[start, end]);
    input.update(cx, |input, _| {
        input
            .admit_edit_positions(&[start, end], &text, &objects)
            .unwrap()
    });
    assert!(matches!(
        input.update(cx, |input, cx| input.begin_replacement(
            ByteRange::from_u64(0, source.len() as u64).unwrap(),
            "x".to_owned(),
            crate::MutationKind::Edit,
            cx,
        )),
        Err(RangeTextInputError::Pending)
    ));
    service_one_resident_advance(&input, cx);
    input.read_with(cx, |input, _| {
        assert!(input.replacement.is_some());
        assert_eq!(input.response_custody.len(), 1);
    });
    let later_advances = drain_resident_advances(&input, cx);
    assert_many_bounded_advances(later_advances + 1);
    input.read_with(cx, |input, _| {
        assert!(input.replacement.is_none());
        assert!(
            input
                .requests
                .iter()
                .any(|request| matches!(request, RangeTextInputRequest::MutationBegin(_)))
        );
    });
    dispose_resident_chain(&input, cx);
}

#[gpui::test]
fn resident_clipboard_collection_advances_one_page_per_frame(cx: &mut gpui::TestAppContext) {
    let (source, input, cx) = new_resident_chain(cx);
    let start = SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::no_objects());
    let end = SourcePosition::new(
        ByteOffset::new(source.len() as u64),
        crate::InlineObjectGap::no_objects(),
    );
    let (_, text, objects) = admitted_successor_sources(&source, 1, &[start, end]);
    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                crate::ClipboardKind::Copy,
                crate::SourceRange::new(start, end).unwrap(),
                crate::MutationPositions::new(end, start, end),
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let object_request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            request => panic!("unexpected clipboard setup request: {request:?}"),
        }
    };
    let object_page = ObjectPage::new(
        ObjectPageId::new(400_000),
        object_request.key(),
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            input
                .deliver_object_page_in_window(object_page, window, cx)
                .unwrap();
            assert_eq!(input.realization_diagnostics().frame.spent, 1);
            assert_eq!(input.response_custody.len(), 1);
        })
    });
    service_one_resident_advance(&input, cx);
    input.read_with(cx, |input, _| {
        assert_eq!(input.response_custody.len(), 1);
        assert!(input.clipboard.pending_text_page().is_some());
    });
    let later_advances = drain_resident_advances(&input, cx);
    assert_many_bounded_advances(later_advances + 1);
    let write = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::ClipboardWrite(write) => break write,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            request => panic!("unexpected clipboard completion request: {request:?}"),
        }
    };
    assert_eq!(write.text(), source);
    dispose_resident_chain(&input, cx);
}

#[gpui::test]
fn direct_clipboard_object_and_text_prepare_one_under_retry_then_commit_once(
    cx: &mut gpui::TestAppContext,
) {
    let source = continuation_source();
    let configuration = continuation_config(&source);
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    drive_surface_for_source(&input, cx, &source);
    let start = SourcePosition::new(ByteOffset::new(0), crate::InlineObjectGap::no_objects());
    let end = SourcePosition::new(
        ByteOffset::new(source.len() as u64),
        crate::InlineObjectGap::no_objects(),
    );
    let (_, text, objects) = admitted_successor_sources(&source, 1, &[start, end]);
    input.update(cx, |input, cx| {
        input
            .begin_composite_clipboard(
                crate::ClipboardKind::Copy,
                crate::SourceRange::new(start, end).unwrap(),
                crate::MutationPositions::new(end, start, end),
                &text,
                &objects,
                cx,
            )
            .unwrap();
    });
    let object_request = loop {
        match input.update(cx, |input, _| input.take_request()).unwrap() {
            RangeTextInputRequest::ObjectPage(request) => break request,
            RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) => {
            }
            request => panic!("unexpected clipboard object setup request: {request:?}"),
        }
    };
    let object_key = object_request.key();
    let object_page = ObjectPage::new(
        ObjectPageId::new(410_000),
        object_key,
        vec![],
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let object_exact = input.update(cx, |input, _| {
        input
            .admit_response_custody(
                super::super::response_custody::RangeResponseCustody::Object(object_page.clone()),
            )
            .unwrap();
        let step = input.clipboard.prepare_object_page(&object_page).unwrap();
        let current = input.current_realization_ownership();
        let old = input.clipboard.ownership_charge();
        let retained = object_page.retained_charge();
        let processing_items = retained.allocated_items().checked_add(1).unwrap();
        let service_current = RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_sub(retained.bytes() - std::mem::size_of::<ObjectPage>())
                .and_then(|value| value.checked_add(retained.bytes()))
                .unwrap(),
            items: current
                .owned_items
                .checked_sub(retained.allocated_items())
                .and_then(|value| value.checked_add(processing_items))
                .unwrap(),
        };
        let transfer = if step.transfers_response() {
            RangeSurfaceCharge {
                bytes: retained.bytes(),
                items: processing_items,
            }
        } else {
            RangeSurfaceCharge::default()
        };
        let projected = RangeSurfaceCharge {
            bytes: service_current
                .bytes
                .checked_sub(old.bytes())
                .and_then(|value| value.checked_sub(transfer.bytes))
                .and_then(|value| value.checked_add(step.peak_ownership().bytes()))
                .unwrap(),
            items: service_current
                .items
                .checked_sub(old.items())
                .and_then(|value| value.checked_sub(transfer.items))
                .and_then(|value| value.checked_add(step.peak_ownership().items()))
                .unwrap(),
        };
        RangeSurfaceCharge {
            bytes: service_current.bytes.max(projected.bytes),
            items: service_current.items.max(projected.items),
        }
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_surface_bytes = object_exact.bytes - 1;
            input.config.limits.max_surface_items = object_exact.items;
            input.begin_realization_frame();
            assert!(matches!(
                input.service_response_custody(window, cx),
                super::super::response_custody::ResponseCustodyProgress::RetryableClipboardPreparationCapacity
            ));
            assert!(input.dispatched_object_pages.contains(&object_key));
            assert!(matches!(
                input.response_custody.front(),
                Some(super::super::response_custody::RangeResponseCustody::Object(page)) if page.key() == object_key
            ));
            assert!(input.realization_continuation_scheduled);
            input.config.limits.max_surface_bytes = object_exact.bytes;
            input.begin_realization_frame();
            assert!(matches!(
                input.service_response_custody(window, cx),
                super::super::response_custody::ResponseCustodyProgress::Progressed
                    | super::super::response_custody::ResponseCustodyProgress::RetryableClipboardPreparationCapacity
            ));
            assert!(!input.dispatched_object_pages.contains(&object_key));
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(key) if *key == object_key))
                    .count(),
                1
            );
        })
    });
    let text_request = loop {
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request))
                if request.key().purpose() == PagePurpose::Clipboard =>
            {
                break request;
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_)) => {}
            Some(request) => panic!("unexpected clipboard text setup request: {request:?}"),
            None => cx.update(|window, app| {
                input.update(app, |input, cx| {
                    input.config.limits.max_surface_bytes = 8 * 1024 * 1024;
                    input.config.limits.max_surface_items = 65_536;
                    input.begin_realization_frame();
                    assert!(!matches!(
                        input.service_response_custody(window, cx),
                        super::super::response_custody::ResponseCustodyProgress::Rejected(_)
                    ));
                })
            }),
        }
    };
    let text_key = text_request.key();
    let text_page = page_for_source(text_request, 410_001, &source);
    let text_exact = input.update(cx, |input, _| {
        input.config.limits.max_surface_bytes = 8 * 1024 * 1024;
        input.config.limits.max_surface_items = 65_536;
        input
            .admit_response_custody(
                super::super::response_custody::RangeResponseCustody::PageNoAliases(
                    text_page.clone(),
                ),
            )
            .unwrap();
        let index = input
            .response_custody
            .iter()
            .position(|response| {
                matches!(
                    response,
                    super::super::response_custody::RangeResponseCustody::PageNoAliases(page)
                        if page.key() == text_key
                )
            })
            .unwrap();
        let response = input.response_custody.remove(index).unwrap();
        input.response_custody.push_front(response);
        let step = input.clipboard.prepare_text_page(&text_page).unwrap();
        let current = input.current_realization_ownership();
        let old = input.clipboard.ownership_charge();
        let retained = text_page.retained_charge();
        let service_current = RangeSurfaceCharge {
            bytes: current
                .owned_bytes
                .checked_sub(retained.bytes() - std::mem::size_of::<RangePage>())
                .and_then(|value| value.checked_add(retained.bytes()))
                .unwrap(),
            items: current
                .owned_items
                .checked_sub(retained.items().saturating_sub(1))
                .and_then(|value| value.checked_add(retained.items()))
                .unwrap(),
        };
        let transfer = if step.transfers_response() {
            RangeSurfaceCharge {
                bytes: retained.bytes(),
                items: retained.items(),
            }
        } else {
            RangeSurfaceCharge::default()
        };
        let projected = RangeSurfaceCharge {
            bytes: service_current
                .bytes
                .checked_sub(old.bytes())
                .and_then(|value| value.checked_sub(transfer.bytes))
                .and_then(|value| value.checked_add(step.peak_ownership().bytes()))
                .unwrap(),
            items: service_current
                .items
                .checked_sub(old.items())
                .and_then(|value| value.checked_sub(transfer.items))
                .and_then(|value| value.checked_add(step.peak_ownership().items()))
                .unwrap(),
        };
        RangeSurfaceCharge {
            bytes: service_current.bytes.max(projected.bytes),
            items: service_current.items.max(projected.items),
        }
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.config.limits.max_surface_bytes = text_exact.bytes - 1;
            input.config.limits.max_surface_items = text_exact.items;
            input.begin_realization_frame();
            assert!(matches!(
                input.service_response_custody(window, cx),
                super::super::response_custody::ResponseCustodyProgress::RetryableClipboardPreparationCapacity
            ));
            assert!(input.dispatched_pages.contains(&text_key));
            assert!(matches!(
                input.response_custody.front(),
                Some(super::super::response_custody::RangeResponseCustody::PageNoAliases(page)) if page.key() == text_key
            ));
            assert!(input.realization_continuation_scheduled);
            input.config.limits.max_surface_bytes = text_exact.bytes;
            input.begin_realization_frame();
            assert!(matches!(
                input.service_response_custody(window, cx),
                super::super::response_custody::ResponseCustodyProgress::Progressed
                    | super::super::response_custody::ResponseCustodyProgress::RetryableClipboardPreparationCapacity
            ));
            input.config.limits.max_surface_bytes = 8 * 1024 * 1024;
            input.config.limits.max_surface_items = 65_536;
            for _ in 0..64 {
                if !input.dispatched_pages.contains(&text_key) {
                    break;
                }
                input.begin_realization_frame();
                assert!(!matches!(
                    input.service_response_custody(window, cx),
                    super::super::response_custody::ResponseCustodyProgress::Rejected(_)
                ));
            }
            assert!(!input.dispatched_pages.contains(&text_key));
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| matches!(request, RangeTextInputRequest::ReleasePage(key) if *key == text_key))
                    .count(),
                1
            );
        })
    });
}

#[gpui::test]
fn resident_continuation_disposal_cancels_bounded_pending_state(cx: &mut gpui::TestAppContext) {
    let (source, input, cx) = new_resident_chain(cx);
    let _ = input.update(cx, |input, cx| {
        input.platform_text_for_range(0..source.len(), cx).unwrap()
    });
    input.read_with(cx, |input, _| {
        assert_eq!(input.response_custody.len(), 1);
        assert!(input.realization_continuation_scheduled);
    });
    dispose_resident_chain(&input, cx);
    input.read_with(cx, |input, _| {
        assert!(!input.realization_continuation_scheduled);
        let current = input.realization_diagnostics().current;
        assert_eq!(current.resident_pages, 0);
        assert_eq!(current.resident_page_bytes, 0);
        assert_eq!(current.request_storage_bytes, 0);
    });
}
