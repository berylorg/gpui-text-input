use super::*;
use crate::{ByteOffset, InlineObjectGap, InlineObjectNeighbor, SourcePosition};

#[gpui::test]
fn predecessor_retry_cancellation_drops_custody_and_retires_exact_request_once(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(
            super::super::terminal_tests::config(2 * 1024 * 1024, 32768),
            window,
            cx,
        )
        .unwrap()
    });
    super::super::terminal_tests::drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let (mutation, key) = begin_replacement_response(input, "x".to_owned(), cx);
        input.admit_response_custody(super::super::response_custody::RangeResponseCustody::Object(response_page(key, Vec::new()))).unwrap();
        let current = input.current_realization_ownership();
        input.config.limits.max_surface_bytes = current.owned_bytes + std::mem::size_of::<ObjectPage>()
            + 4 * std::mem::size_of::<MutationPageItem>() - 1;
        input.begin_realization_frame();
        assert!(matches!(input.service_object_response_custody(cx),
            super::super::response_custody::ResponseCustodyProgress::RetryableMutationPreparationCapacity));
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
        input.cancel_mutation(mutation, cx).unwrap();
        assert!(input.response_custody.is_empty());
        assert!(input.pending_local_mutation.is_none());
        assert!(!input.dispatched_object_pages.contains(&key));
        assert_eq!(input.requests.iter().filter(|request| matches!(request, RangeTextInputRequest::CancelObjectPage(found) if *found == key)).count(), 1);
        assert!(!input.requests.iter().any(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(found) if *found == key)));
        assert!(matches!(input.deliver_object_page(response_page(key, Vec::new()), cx),
            Err(RangeTextInputError::ObjectResponseRejected(_))));
    });
}

#[gpui::test]
fn proven_small_plain_replacement_uses_exact_single_page_without_predecessor_reads(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(
            super::super::terminal_tests::config(2 * 1024 * 1024, 32768),
            window,
            cx,
        )
        .unwrap()
    });
    super::super::terminal_tests::drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let position = |offset| SourcePosition::new(ByteOffset::new(offset), InlineObjectGap::NoObjects);
        let range = SourceRange::new(position(1), position(3)).unwrap();
        let binding = input.config.binding;
        let key = input.begin_composite_replacement(range, "Q".to_owned(), crate::MutationKind::Edit, cx).unwrap();
        assert!(input.replacement.is_none());
        assert!(input.pending_local_mutation.as_ref().unwrap().producer.is_none());
        assert!(matches!(input.take_request(), Some(RangeTextInputRequest::MutationBegin(_))));
        let pass = input.request_mutation_evidence(key).unwrap();
        let crate::LocalMutationEvidence::Page(page) = input.local_mutation_evidence(pass, cx).unwrap() else { panic!("single evidence page") };
        let page = page.clone();
        assert!(matches!(&page.items()[0], MutationPageItem::Utf8 { text, .. } if text.as_ref() == "Q"));
        let ack = input.submit_mutation_evidence_page(pass, page, cx).unwrap();
        input.acknowledge_mutation_evidence_page(ack).unwrap();
        let crate::LocalMutationEvidence::Finish(finish) = input.local_mutation_evidence(pass, cx).unwrap() else { panic!("exact evidence EOF") };
        assert_eq!(finish.intended(), MutationPositionsIntent::Exact(MutationPositions::collapsed(position(2))));
        assert!(!input.requests.iter().any(|request| matches!(request, RangeTextInputRequest::ObjectPage(request) if request.key().purpose() == ObjectPurpose::MutationPredecessor)));
        assert_eq!(input.config.binding, binding);
        input.cancel_mutation(key, cx).unwrap();
    });
}

fn begin_replacement_subject(
    input: &mut RangeTextInput,
    text: String,
    cx: &mut gpui::Context<RangeTextInput>,
) -> MutationKey {
    input.config.mutation_limits = crate::MutationLimits::new(2, 8192).unwrap();
    let range = SourceRange::new(
        SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects),
        SourcePosition::new(
            ByteOffset::new(input.config.binding.extent().byte_len()),
            InlineObjectGap::NoObjects,
        ),
    )
    .unwrap();
    let surface = input.surface.as_ref().unwrap();
    let proofs = [range.start(), range.end()].map(|position| {
        Some(
            crate::range_edit::SourcePositionProof::from_surface_pages(
                input.config.binding,
                position,
                surface.pages(),
                surface.object_pages(),
            )
            .unwrap(),
        )
    });
    let key = input
        .begin_paged_source_replacement(
            range,
            MutationPositions::collapsed(range.end()),
            proofs,
            0,
            text,
            crate::MutationKind::Edit,
            cx,
        )
        .unwrap();
    assert!(matches!(
        input.take_request(),
        Some(RangeTextInputRequest::MutationBegin(_))
    ));
    key
}

fn begin_replacement_response(
    input: &mut RangeTextInput,
    text: String,
    cx: &mut gpui::Context<RangeTextInput>,
) -> (MutationKey, ObjectRequestKey) {
    let key = begin_replacement_subject(input, text, cx);
    let pass = input.request_mutation_evidence(key).unwrap();
    assert!(matches!(
        input.local_mutation_evidence(pass, cx).unwrap(),
        crate::LocalMutationEvidence::Pending
    ));
    let Some(RangeTextInputRequest::ObjectPage(request)) = input.take_request() else {
        panic!("predecessor object request");
    };
    (key, request.key())
}

#[gpui::test]
fn staging_page_survives_logical_and_physical_queue_saturation_then_transfers_at_exact_capacity(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(
            super::super::terminal_tests::config(2 * 1024 * 1024, 32768),
            window,
            cx,
        )
        .unwrap()
    });
    super::super::terminal_tests::drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let key = begin_replacement_subject(input, "x".to_owned(), cx);
        input.accept_mutation_preflight(key, cx).unwrap();
        let Some(RangeTextInputRequest::ObjectPage(request)) = input.take_request() else {
            panic!("staging predecessor object request");
        };
        let object_key = request.key();
        input.pending_local_mutation.as_mut().unwrap().producer.as_mut().unwrap().mode = ProducerMode::Dormant;
        input.admit_response_custody(super::super::response_custody::RangeResponseCustody::Object(response_page(object_key, Vec::new()))).unwrap();
        input.begin_realization_frame();
        assert!(matches!(input.service_object_response_custody(cx),
            super::super::response_custody::ResponseCustodyProgress::Progressed));
        assert!(input.response_custody.is_empty());
        input.pending_local_mutation.as_mut().unwrap().producer.as_mut().unwrap().mode = ProducerMode::Staging(None);
        input.prepare_local_replacement(cx).unwrap();
        let page = input.pending_local_mutation.as_ref().unwrap().page.as_ref().unwrap().clone();
        input.submit_mutation_page(page.clone(), cx).unwrap();
        assert_eq!(input.submit_mutation_page(page, cx).unwrap(), crate::MutationPageAcceptance::Replay);
        assert_eq!(input.queued_mutation_requests(key), 2);
        assert!(input.requests.len() < input.requests.capacity());
        let local = input.pending_local_mutation.as_ref().unwrap();
        let page_identity = local.page.as_ref().unwrap().page_identity();
        let frontier = local.producer.as_ref().map(|producer|
            (producer.next_cursor, producer.next_ordinal, producer.cumulative, producer.totals)).unwrap();
        assert!(matches!(input.drive_local_replacement(cx), Err(RangeTextInputError::Busy)));
        assert_eq!(input.pending_local_mutation.as_ref().unwrap().page.as_ref().unwrap().page_identity(), page_identity);
        let mut releases = 0;
        while let Some(request) = input.take_request() {
            if matches!(request, RangeTextInputRequest::ReleaseObjectPage(found) if found == object_key) { releases += 1; }
        }
        assert_eq!(releases, 1);
        for index in 0..input.requests.capacity() {
            let padding = ObjectRequestKey::new(ObjectRequestId::new(90_000 + index as u64),
                object_key.binding(), object_key.revision(), object_key.presentation_generation(),
                ObjectPurpose::Clipboard, object_key.demand()).unwrap();
            input.commit_prepared_request(RangeTextInputRequest::ReleaseObjectPage(padding));
        }
        assert!(matches!(input.drive_local_replacement(cx), Err(RangeTextInputError::Busy)));
        let local = input.pending_local_mutation.as_ref().unwrap();
        assert_eq!(local.page.as_ref().unwrap().page_identity(), page_identity);
        let producer = local.producer.as_ref().unwrap();
        assert_eq!((producer.next_cursor, producer.next_ordinal, producer.cumulative, producer.totals), frontier);
        while input.take_request().is_some() {}
        let exact = input.current_realization_ownership();
        input.config.limits.max_surface_bytes = exact.owned_bytes;
        input.config.limits.max_surface_items = exact.owned_items;
        input.drive_local_replacement(cx).unwrap();
        assert!(input.pending_local_mutation.as_ref().unwrap().page.is_none());
        assert_eq!(input.requests.iter().filter(|request| matches!(request, RangeTextInputRequest::MutationProposalPage(request) if request.page().page_identity() == page_identity)).count(), 1);
        assert_eq!(input.requests.iter().filter(|request| matches!(request, RangeTextInputRequest::MutationFinishInput(finish) if finish.key() == key)).count(), 1);
        assert!(input.current_realization_ownership().owned_bytes <= exact.owned_bytes);
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
        input.config.limits.max_surface_items = 32768;
        input.cancel_mutation(key, cx).unwrap();
        assert!(input.pending_local_mutation.is_none());
    });
}

fn response_page(key: ObjectRequestKey, objects: Vec<crate::InlineObjectFact>) -> ObjectPage {
    ObjectPage::new(
        crate::ObjectPageId::new(99),
        key,
        objects,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap()
}

#[gpui::test]
fn predecessor_response_one_under_preserves_single_custody_and_exact_retry_releases_once(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(
            super::super::terminal_tests::config(2 * 1024 * 1024, 32768),
            window,
            cx,
        )
        .unwrap()
    });
    super::super::terminal_tests::drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        let original_binding = input.config.binding;
        let (mutation, key) = begin_replacement_response(input, String::new(), cx);
        let fact = crate::InlineObjectFact::new(crate::InlineObjectId::new(1), ByteOffset::new(1),
            crate::InlineObjectOrder::new(1), "object", crate::InlineObjectPresentation::new(
                1, "", gpui::px(8.), gpui::px(8.), gpui::Pixels::ZERO, None, 0, false).unwrap());
        let page = response_page(key, vec![fact]);
        let retained = page.retained_charge();
        input.admit_response_custody(super::super::response_custody::RangeResponseCustody::Object(page)).unwrap();
        let current = input.current_realization_ownership();
        let preparation = 4 * std::mem::size_of::<MutationPageItem>();
        let exact = current.owned_bytes + std::mem::size_of::<ObjectPage>() + preparation;
        let frontier = input.pending_local_mutation.as_ref().unwrap().producer.as_ref().map(|producer|
            (producer.next_cursor, producer.next_ordinal, producer.cumulative, producer.totals)).unwrap();
        input.config.limits.max_surface_bytes = exact - 1;
        input.begin_realization_frame();
        assert!(matches!(input.service_object_response_custody(cx),
            super::super::response_custody::ResponseCustodyProgress::RetryableMutationPreparationCapacity));
        assert!(input.dispatched_object_pages.contains(&key));
        assert!(input.local_replacement_response_is_current(key));
        assert!(matches!(input.response_custody.front(),
            Some(super::super::response_custody::RangeResponseCustody::Object(page)) if page.key() == key && page.retained_charge() == retained));
        let producer = input.pending_local_mutation.as_ref().unwrap().producer.as_ref().unwrap();
        assert_eq!((producer.next_cursor, producer.next_ordinal, producer.cumulative, producer.totals), frontier);
        assert!(input.pending_local_mutation.as_ref().unwrap().page.is_none());
        assert!(!input.requests.iter().any(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(found) if *found == key)));
        input.config.limits.max_surface_bytes = exact;
        input.begin_realization_frame();
        assert!(matches!(input.service_object_response_custody(cx),
            super::super::response_custody::ResponseCustodyProgress::Progressed));
        assert!(input.response_custody.is_empty());
        assert!(!input.dispatched_object_pages.contains(&key));
        assert!(!input.local_replacement_response_is_current(key));
        assert_eq!(input.active_response_processing, RangeSurfaceCharge::default());
        assert_eq!(input.requests.iter().filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(found) if *found == key)).count(), 1);
        assert_eq!(input.config.binding, original_binding);
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
        input.cancel_mutation(mutation, cx).unwrap();
        assert!(input.pending_local_mutation.is_none());
    });
}

#[gpui::test]
fn predecessor_final_payload_is_dropped_before_exact_text_creation(cx: &mut gpui::TestAppContext) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(
            super::super::terminal_tests::config(2 * 1024 * 1024, 32768),
            window,
            cx,
        )
        .unwrap()
    });
    super::super::terminal_tests::drive_initial_surface(&input, cx);
    cx.update(|window, app| input.update(app, |input, cx| {
        let (mutation, key) = begin_replacement_response(input, "x".repeat(4096), cx);
        let baseline = input.current_realization_ownership();
        let page = response_page(key, Vec::with_capacity(16));
        assert!(page.retained_charge().bytes() > 1024);
        let exact = baseline.owned_bytes + 2 * std::mem::size_of::<MutationPageItem>() + 4096;
        input.config.limits.max_surface_bytes = exact;
        input.admit_response_custody(super::super::response_custody::RangeResponseCustody::Object(page)).unwrap();
        input.begin_realization_frame();
        assert!(matches!(input.service_response_custody(window, cx),
            super::super::response_custody::ResponseCustodyProgress::Progressed));
        assert!(input.response_custody.is_empty());
        assert_eq!(input.active_response_processing, RangeSurfaceCharge::default());
        let page = input.pending_local_mutation.as_ref().unwrap().page.as_ref().unwrap();
        assert!(matches!(&page.items()[0], MutationPageItem::Utf8 { text, .. } if text.len() == 4096));
        assert!(input.current_realization_ownership().owned_bytes <= exact);
        assert!(!input.dispatched_object_pages.contains(&key));
        assert_eq!(input.requests.iter().filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(found) if *found == key)).count(), 1);
        input.config.limits.max_surface_bytes = 2 * 1024 * 1024;
        input.cancel_mutation(mutation, cx).unwrap();
    }));
}

#[gpui::test]
fn local_replacement_begin_payload_exact_fit_accepts_and_one_under_is_inert(
    cx: &mut gpui::TestAppContext,
) {
    for shortage in [1, 0] {
        let (input, cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(
                super::super::terminal_tests::config(2 * 1024 * 1024, 32768),
                window,
                cx,
            )
            .unwrap()
        });
        super::super::terminal_tests::drive_initial_surface(&input, cx);
        input.update(cx, |input, cx| {
            let range = SourceRange::new(
                SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects),
                SourcePosition::new(ByteOffset::new(input.config.binding.extent().byte_len()), InlineObjectGap::NoObjects),
            ).unwrap();
            let surface = input.surface.as_ref().unwrap();
            let proofs = [range.start(), range.end()].map(|position| Some(
                crate::range_edit::SourcePositionProof::from_surface_pages(input.config.binding, position, surface.pages(), surface.object_pages()).unwrap(),
            ));
            let predecessor = MutationPositions::collapsed(range.start());
            let before = input.current_realization_ownership();
            let original_binding = input.config.binding;
            let text = "Q".to_owned();
            input.config.limits.max_surface_bytes = before.owned_bytes + text.capacity() - shortage;
            input.config.limits.max_surface_items = before.owned_items + 1;
            let result = input.begin_paged_source_replacement(range, predecessor, proofs, 0, text, crate::MutationKind::Edit, cx);
            if shortage != 0 {
                assert!(matches!(result, Err(RangeTextInputError::SurfaceCapacity)));
                assert!(input.pending_local_mutation.is_none());
                assert!(input.take_request().is_none());
                assert_eq!(input.current_realization_ownership().owned_bytes, before.owned_bytes);
                assert!(input.is_semantically_quiescent());
            } else {
                let key = result.unwrap();
                let local = input.pending_local_mutation.as_ref().unwrap();
                assert!(local.page.is_none());
                assert_eq!(local.producer.as_ref().unwrap().text, "Q");
                assert_eq!(input.current_realization_ownership().owned_bytes, before.owned_bytes + 1);
                assert!(matches!(input.take_request(), Some(RangeTextInputRequest::MutationBegin(begin)) if begin.proposal().key() == key));
            }
            assert_eq!(input.config.binding, original_binding);
        });
    }
}

#[gpui::test]
fn local_replacement_payload_exact_fit_accepts_and_one_under_is_inert(
    cx: &mut gpui::TestAppContext,
) {
    for shortage in [1, 0] {
        let (input, cx) = cx.add_window_view(|window, cx| {
            RangeTextInput::new(
                super::super::terminal_tests::config(2 * 1024 * 1024, 32768),
                window,
                cx,
            )
            .unwrap()
        });
        super::super::terminal_tests::drive_initial_surface(&input, cx);
        input.update(cx, |input, cx| {
            let position = SourcePosition::new(ByteOffset::new(0), InlineObjectGap::NoObjects);
            let range = SourceRange::new(position, position).unwrap();
            let binding = input.config.binding;
            let key = MutationKey::new(binding.binding(), binding.revision(), crate::OperationId::new(991));
            let empty = MutationStreamFinish { next_cursor: MutationCursor::new(0), next_ordinal: 0, cumulative_identity: MutationIdentity::ROOT, totals: MutationTotals::default() };
            let finish = MutationFinishInput::new(key, empty, empty, binding.extent(), MutationPositions::collapsed(position));
            let mut producer = LocalReplacementProducer::new(range, "X".to_owned());
            producer.complete = true;
            producer.mode = ProducerMode::Staging(None);
            input.pending_local_mutation = Some(super::super::interaction::PendingLocalMutation {
                key, page: None, finish, producer: Some(producer), evidence_page_acknowledged: false,
            });
            let before = input.current_realization_ownership();
            let preparation_bytes = 2 * std::mem::size_of::<MutationPageItem>() + 1;
            input.config.limits.max_surface_bytes = before.owned_bytes + preparation_bytes - shortage;
            input.config.limits.max_surface_items = before.owned_items + 3;
            let result = input.prepare_local_replacement(cx);
            if shortage != 0 {
                assert!(matches!(result, Err(RangeTextInputError::SurfaceCapacity)));
                assert!(input.pending_local_mutation.as_ref().unwrap().page.is_none());
                assert!(!input.pending_local_mutation.as_ref().unwrap().producer.as_ref().unwrap().text_emitted);
                assert_eq!(input.current_realization_ownership().owned_bytes, before.owned_bytes);
                assert_eq!(input.current_realization_ownership().owned_items, before.owned_items);
            } else {
                result.unwrap();
                let page = input.pending_local_mutation.as_ref().unwrap().page.as_ref().unwrap();
                assert!(matches!(&page.items()[0], MutationPageItem::Utf8 { text, .. } if text.as_ref() == "X"));
                assert!(input.current_realization_ownership().owned_bytes <= input.config.limits.max_surface_bytes);
            }
        });
    }
}

#[test]
fn byte_empty_composite_replacement_preserves_both_surviving_neighbors() {
    let neighbors = (1..=4)
        .map(|id| {
            InlineObjectNeighbor::new(
                crate::InlineObjectId::new(id),
                crate::InlineObjectOrder::new(id),
            )
        })
        .collect::<Vec<_>>();
    let range = SourceRange::new(
        SourcePosition::new(
            ByteOffset::new(6),
            InlineObjectGap::between(neighbors[0], neighbors[1]).unwrap(),
        ),
        SourcePosition::new(
            ByteOffset::new(6),
            InlineObjectGap::between(neighbors[2], neighbors[3]).unwrap(),
        ),
    )
    .unwrap();
    assert!(
        MutationPositionsIntent::ReplacementEnd {
            replacement: range,
            inserted_utf8_bytes: 0
        }
        .matches_successor(MutationPositions::collapsed(SourcePosition::new(
            ByteOffset::new(6),
            InlineObjectGap::between(neighbors[0], neighbors[3]).unwrap()
        )))
    );
    assert!(
        MutationPositionsIntent::ReplacementEnd {
            replacement: range,
            inserted_utf8_bytes: 1
        }
        .matches_successor(MutationPositions::collapsed(SourcePosition::new(
            ByteOffset::new(7),
            InlineObjectGap::before(neighbors[3])
        )))
    );
    let facts = neighbors
        .iter()
        .map(|neighbor| {
            crate::InlineObjectFact::new(
                neighbor.id(),
                ByteOffset::new(6),
                neighbor.order(),
                "object".to_owned(),
                crate::InlineObjectPresentation::new(
                    1,
                    gpui::SharedString::new_static(""),
                    gpui::px(8.0),
                    gpui::px(8.0),
                    gpui::Pixels::ZERO,
                    None,
                    0,
                    false,
                )
                .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let key = ObjectRequestKey::new(
        ObjectRequestId::new(1),
        crate::BindingId::new(1),
        crate::SourceRevision::new(1),
        crate::PresentationGeneration::new(1),
        ObjectPurpose::MutationPredecessor,
        ObjectDemandEnvelope::range(
            crate::ByteRange::from_u64(6, 6).unwrap(),
            None,
            ObjectDirection::Forward,
            4,
            8192,
        )
        .unwrap(),
    )
    .unwrap();
    let page = ObjectPage::new(
        crate::ObjectPageId::new(1),
        key,
        facts,
        ObjectPageEdgeFact::EnvelopeBoundary,
        ObjectPageEdgeFact::EnvelopeBoundary,
        true,
        None,
    )
    .unwrap();
    let producer = LocalReplacementProducer::new(range, String::new());
    let prepared = prepare_removals(&producer, &page, 4).unwrap();
    assert!(prepared.complete);
    assert_eq!(prepared.items.len(), 2);
    assert!(
        matches!(prepared.items[0], MutationPageItem::Object(ObjectChange::Remove { target }) if target.id() == neighbors[1].id() && target.range().start() == range.start())
    );
    assert!(
        matches!(prepared.items[1], MutationPageItem::Object(ObjectChange::Remove { target }) if target.id() == neighbors[2].id() && target.range().end() == range.end())
    );
}
