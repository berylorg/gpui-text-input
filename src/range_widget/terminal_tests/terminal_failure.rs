use super::*;

#[derive(Debug, PartialEq)]
struct PreservedTerminalState {
    surface: Option<String>,
    desired: DesiredFingerprint,
    admission: Option<RangeSurfaceCharge>,
    next_id: u64,
    index: Option<String>,
    text_pages: Vec<String>,
    object_pages: Vec<String>,
    active_object: Option<ActiveInlineObject>,
    restoration_seed: Option<crate::RangeRestorationSeed>,
    published_restoration: Option<crate::RangeRestorationSeed>,
    pending_select_all: bool,
}

fn preserved_terminal_state(input: &RangeTextInput) -> PreservedTerminalState {
    PreservedTerminalState {
        surface: input.surface.as_ref().map(|surface| format!("{surface:?}")),
        desired: desired_fingerprint(input.desired),
        admission: input.last_surface_admission,
        next_id: input.next_id,
        index: input.geometry.index().map(|index| format!("{index:?}")),
        text_pages: input
            .residency
            .resident_pages()
            .map(|page| format!("{page:?}"))
            .collect(),
        object_pages: input
            .object_residency
            .resident_pages()
            .map(|page| format!("{page:?}"))
            .collect(),
        active_object: input.active_object,
        restoration_seed: input.restoration_seed,
        published_restoration: input.published_restoration,
        pending_select_all: input.pending_select_all,
    }
}

fn assert_terminal_failure_closed(input: &RangeTextInput) {
    assert!(input.active_geometry.is_none());
    assert!(input.pending_geometry_page.is_none());
    assert!(input.pending_geometry_object.is_none());
    assert!(input.pending_target_intent.is_none());
    assert!(input.response_custody.is_empty());
    assert_eq!(input.residency.pending_requests().count(), 0);
    assert_eq!(input.object_residency.pending_requests().count(), 0);

    let current = input.realization_diagnostics().current;
    assert_eq!(current.active_geometry_jobs, 0);
    assert_eq!(current.pending_geometry_pages, 0);
    assert_eq!(current.pending_geometry_objects, 0);
    assert_eq!(current.pending_page_requests, 0);
    assert_eq!(current.pending_object_requests, 0);
    assert_eq!(current.deferred_geometry_responses, 0);
    assert_eq!(current.response_custody_count, 0);
    assert_eq!(current.pending_target_intents, 0);
}

#[gpui::test]
fn impossible_terminal_object_retarget_closes_once_without_retry(cx: &mut gpui::TestAppContext) {
    let source = "line\n".repeat(24);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(231),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 24),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 512 * 1024, 8192).unwrap();
    configuration.limits.page_bytes = source.len() as u64;
    configuration.limits.max_realized_block_extent = px(80.);
    configuration.viewport_extent = px(160.);
    configuration.overscan = Pixels::ZERO;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let key = stage_terminal_target_object_response(&input, cx, &source);
    let before = input.update(cx, |input, _| {
        let current = input.desired.source_selection.unwrap().head;
        input.restoration_seed = Some(crate::RangeRestorationSeed {
            binding: input.config.binding,
            caret: current,
            selection: RangeSourceSelection::caret(current),
            scroll: crate::RangeRestorationScrollAnchor {
                position: current,
                intra_anchor: Pixels::ZERO,
            },
            history: None,
        });
        let endpoint = SourcePosition::new(
            ByteOffset::new(source.len() as u64 + 1),
            crate::InlineObjectGap::no_objects(),
        );
        let candidate = input.surface_candidate.as_mut().unwrap();
        candidate.desired.source_selection = Some(RangeSourceSelection::caret(endpoint));
        candidate.desired.reveal_caret = true;
        preserved_terminal_state(input)
    });
    let result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_object_page_in_window(empty_terminal_object_response(key), window, cx)
        })
    });
    assert!(result.is_ok(), "{result:?}");
    input.update(cx, |input, _| {
        assert_eq!(preserved_terminal_state(input), before);
        assert!(input.surface_candidate.is_none());
        assert_terminal_failure_closed(input);
        assert!(!input.dispatched_object_pages.contains(&key));
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                .count(),
            1
        );
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            assert!(custody_idle(input.service_response_custody(window, cx)));
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::ReleaseObjectPage(released)) if released == key
            ));
            assert!(input.take_request().is_none());
            assert_terminal_failure_closed(input);
            assert_eq!(
                input
                    .realization_diagnostics()
                    .current
                    .dispatched_object_requests,
                0
            );
        })
    });
}

#[gpui::test]
fn delivered_text_then_resident_terminal_incomplete_surface_closes_once(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(20);
    let mut configuration = super::seal::presentation_config(&source);
    configuration.binding = RangeBinding::new(
        BindingId::new(232),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 20),
    );
    configuration.geometry_limits = ExactGeometryLimits::new(4, 8, 512 * 1024, 8192).unwrap();
    configuration.residency_limits = ResidencyLimits::new(1, 128 * 1024, 8, 256).unwrap();
    configuration.object_residency_limits =
        ObjectResidencyLimits::new(32, 32, 128 * 1024, 64 * 1024, 8, 32, 128 * 1024).unwrap();
    configuration.limits.page_bytes = 4;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let page = super::seal::stage_terminal_target_text_response(&input, cx, &source);
    let key = page.key();
    input.update(cx, |input, _| {
        input
            .surface_candidate
            .as_mut()
            .unwrap()
            .desired
            .inline_object_interaction = Some(DesiredInlineObjectInteraction::Set {
            object_id: InlineObjectId::new(900_001),
            order: InlineObjectOrder::new(1),
            activation_eligible: true,
            origin: None,
        });
    });
    cx.update(|window, app| input.update(app, |input, cx| input.deliver_page(page, window, cx)))
        .unwrap();
    let (before, object_key) = input.read_with(cx, |input, _| {
        (
            preserved_terminal_state(input),
            input
                .pending_geometry_object
                .as_ref()
                .expect("terminal text delivery advances to resident object validation")
                .request
                .key(),
        )
    });
    let result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            input.service_geometry_page(window, cx)
        })
    });
    assert!(
        matches!(result, Err(RangeTextInputError::IncompleteSurface)),
        "{result:?}"
    );
    input.update(cx, |input, _| {
        assert_eq!(preserved_terminal_state(input), before);
        assert!(input.surface_candidate.is_none());
        assert_terminal_failure_closed(input);
        assert!(!input.dispatched_pages.contains(&key));
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleasePage(released) if *released == key))
                .count(),
            1
        );
        assert!(!input.requests.iter().any(
            |request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == object_key)
        ));
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            assert!(custody_idle(input.service_response_custody(window, cx)));
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::ReleasePage(released)) if released == key
            ));
            assert!(input.take_request().is_none());
            assert_terminal_failure_closed(input);
            assert_eq!(
                input
                    .realization_diagnostics()
                    .current
                    .dispatched_page_requests,
                0
            );
        })
    });
}

#[gpui::test]
fn ordinary_terminal_object_publication_still_succeeds(cx: &mut gpui::TestAppContext) {
    let source = "line\n".repeat(24);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(233),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 24),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 512 * 1024, 8192).unwrap();
    configuration.limits.page_bytes = source.len() as u64;
    configuration.limits.max_realized_block_extent = px(80.);
    configuration.viewport_extent = px(160.);
    configuration.overscan = Pixels::ZERO;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let key = stage_terminal_target_object_response(&input, cx, &source);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(empty_terminal_object_response(key), window, cx)
                .unwrap();
            assert!(input.active_geometry.is_none());
            assert!(input.pending_geometry_object.is_none());
            assert!(input.surface_candidate.is_none());
            assert!(input.response_custody.is_empty());
            assert!(!input.dispatched_object_pages.contains(&key));
            assert!(input.is_surface_current_and_interactive());
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                    .count(),
                1
            );
        })
    });
}

fn stage_active_coalesced_object_response(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) -> (
    crate::ObjectRequestKey,
    crate::ObjectRequestKey,
    crate::GeometryJobKey,
) {
    drive_surface_for_source(input, cx, source);
    let response_key = input.update(cx, |input, _| {
        let demand = ObjectDemandEnvelope::range(
            ByteRange::from_u64(0, source.len() as u64).unwrap(),
            None,
            ObjectDirection::Forward,
            input.config.object_residency_limits.max_pending_objects(),
            input.config.object_residency_limits.max_pending_bytes(),
        )
        .unwrap();
        let id = ObjectRequestId::new(input.next_id());
        let ObjectDemand::Requested(request) = input
            .object_residency
            .demand(id, ObjectPurpose::GeometryTarget, demand)
            .unwrap()
        else {
            panic!("fresh external response must own one residency request")
        };
        assert!(input.dispatched_object_pages.insert(request.key()));
        request.key()
    });
    let (layout, style) = input.read_with(cx, |input, _| {
        (input.config.layout.clone(), input.config.style.clone())
    });
    input.update(cx, |input, cx| input.set_layout(layout, style, cx).unwrap());
    for page_id in 60_000..60_512 {
        if let Some((pending_key, job)) = input.read_with(cx, |input, _| {
            let pending = input.pending_geometry_object.as_ref()?;
            (input.active_geometry == Some(pending.job)
                && pending.request.key() != response_key
                && matches!(pending.wait, GeometryObjectWait::Coalesced(wait) if wait == response_key)
                && input.dispatched_object_pages.contains(&response_key))
            .then_some((pending.request.key(), pending.job))
        }) {
            return (response_key, pending_key, job);
        }
        cx.update(|window, app| window.draw(app).clear());
        cx.run_until_parked();
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => {
                let page = page_for_source(request, page_id, source);
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                assert_ne!(request.key(), response_key);
                assert_ne!(request.key().purpose(), ObjectPurpose::GeometryTarget);
                let page = ObjectPage::new(
                    ObjectPageId::new(page_id),
                    request.key(),
                    vec![],
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    ObjectPageEdgeFact::EnvelopeBoundary,
                    true,
                    None,
                )
                .unwrap();
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_)) => {}
            Some(RangeTextInputRequest::CancelObjectPage(key)) if key != response_key => {}
            Some(request) => panic!("active coalescing cancelled its source response: {request:?}"),
            None => {}
        }
    }
    panic!("active geometry did not coalesce its logical object demand");
}

#[gpui::test]
fn delivered_text_residency_limit_is_accepted_terminal(cx: &mut gpui::TestAppContext) {
    let source = "line\n".repeat(20);
    let mut configuration = super::seal::presentation_config(&source);
    configuration.binding = RangeBinding::new(
        BindingId::new(235),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 20),
    );
    configuration.geometry_limits = ExactGeometryLimits::new(4, 8, 512 * 1024, 8192).unwrap();
    configuration.residency_limits = ResidencyLimits::new(1, 128 * 1024, 8, 256).unwrap();
    configuration.object_residency_limits =
        ObjectResidencyLimits::new(32, 32, 128 * 1024, 64 * 1024, 8, 32, 128 * 1024).unwrap();
    configuration.limits.page_bytes = 4;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let page = super::seal::stage_terminal_target_text_response(&input, cx, &source);
    let key = page.key();
    input.update(cx, |input, _| input.residency.force_next_admission_limit());
    let result = cx
        .update(|window, app| input.update(app, |input, cx| input.deliver_page(page, window, cx)));
    assert!(result.is_ok(), "{result:?}");
    input.read_with(cx, |input, _| {
        assert!(!input.dispatched_pages.contains(&key));
        assert!(input.response_custody.is_empty());
        assert!(input.active_geometry.is_none());
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleasePage(released) if *released == key))
                .count(),
            1
        );
    });
}

fn ordinary_page_response(
    request: PageRequest,
    id: u64,
    source: &str,
    malformed: bool,
) -> RangePage {
    if !malformed {
        return page_for_source(request, id, source);
    }
    let start = source.len() as u64 - 1;
    RangePage::new(
        PageId::new(id),
        request.key(),
        ByteRange::from_u64(start, start + 2).unwrap(),
        "xx".to_owned(),
        vec![],
        PageEdgeFact::Continues,
        PageEdgeFact::Continues,
        false,
    )
    .unwrap()
}

fn exercise_ordinary_page_admission_failure(
    cx: &mut gpui::TestAppContext,
    purpose: PagePurpose,
    malformed: bool,
    base: u64,
) {
    let source = "0123456789abcdef";
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(base),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 1),
    );
    configuration.residency_limits = ResidencyLimits::new(8, 128 * 1024, 8, 512).unwrap();
    let binding = configuration.binding;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());

    let (page, key, pending_neighbor, resident_neighbor, request_neighbor, custody_neighbor) =
        input.update(cx, |input, _| {
            let resident_demand = PageDemandEnvelope::Adjacent {
                anchor: ByteOffset::new(source.len() as u64),
                direction: PageDirection::Backward,
                max_payload_bytes: 4,
            };
            let PageDemand::Requested(resident_request) = input
                .residency
                .demand(
                    PageRequestId::new(base + 1),
                    PagePurpose::Selection,
                    resident_demand,
                )
                .unwrap()
            else {
                panic!("resident neighbor request")
            };
            let resident_neighbor = PageId::new(base + 1);
            input
                .residency
                .admit(page_for_source(resident_request, base + 1, source))
                .unwrap();

            let neighbor_purpose = match purpose {
                PagePurpose::Viewport => PagePurpose::Caret,
                PagePurpose::Caret => PagePurpose::Viewport,
                _ => unreachable!(),
            };
            let PageDemand::Requested(pending_request) = input
                .residency
                .demand(
                    PageRequestId::new(base + 2),
                    neighbor_purpose,
                    PageDemandEnvelope::Adjacent {
                        anchor: ByteOffset::new(8),
                        direction: PageDirection::Forward,
                        max_payload_bytes: 4,
                    },
                )
                .unwrap()
            else {
                panic!("pending neighbor request")
            };
            let pending_neighbor = pending_request.key();

            let response_demand = if malformed {
                PageDemandEnvelope::Adjacent {
                    anchor: ByteOffset::new(source.len() as u64 - 1),
                    direction: PageDirection::Forward,
                    max_payload_bytes: 4,
                }
            } else {
                PageDemandEnvelope::Adjacent {
                    anchor: ByteOffset::new(0),
                    direction: PageDirection::Forward,
                    max_payload_bytes: 4,
                }
            };
            let PageDemand::Requested(response_request) = input
                .residency
                .demand(PageRequestId::new(base + 3), purpose, response_demand)
                .unwrap()
            else {
                panic!("ordinary response request")
            };
            let key = response_request.key();
            assert!(input.dispatched_pages.insert(key));
            let page = ordinary_page_response(response_request, base + 3, source, malformed);

            let request_neighbor = pending_neighbor;
            input
                .requests
                .push_back(RangeTextInputRequest::CancelPage(request_neighbor));

            let custody_key = crate::PageRequestKey::adjacent(
                PageRequestId::new(base + 4),
                binding.binding(),
                binding.revision(),
                PagePurpose::Selection,
                ByteOffset::new(0),
                PageDirection::Forward,
                4,
            )
            .unwrap();
            let custody_neighbor = page_for_source(PageRequest::new(custody_key), base + 4, source);
            (
                page,
                key,
                pending_neighbor,
                resident_neighbor,
                request_neighbor,
                custody_neighbor,
            )
        });

    let custody_key = custody_neighbor.key();
    let (progress, fresh_page, fresh_key) = cx.update(|window, app| {
        input.update(app, |input, cx| {
            if !malformed {
                input.residency.force_next_admission_limit();
            }
            input
                .admit_response_custody(super::response_custody::RangeResponseCustody::Page(page))
                .unwrap();
            input
                .admit_response_custody(
                    super::response_custody::RangeResponseCustody::PageNoAliases(
                        custody_neighbor,
                    ),
                )
                .unwrap();
            input.begin_realization_frame();
            let progress = input.service_response_custody(window, cx);
            assert!(!input.residency.pending_requests().any(|pending| pending == key));
            assert!(!input.dispatched_pages.contains(&key));
            assert!(input
                .residency
                .pending_requests()
                .any(|pending| pending == pending_neighbor));
            assert!(input
                .residency
                .resident_pages()
                .any(|page| page.id() == resident_neighbor));
            assert!(input.requests.iter().any(
                |request| matches!(request, RangeTextInputRequest::CancelPage(cancelled) if *cancelled == request_neighbor)
            ));
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| matches!(request, RangeTextInputRequest::ReleasePage(released) if *released == key))
                    .count(),
                1
            );
            assert!(!input.response_custody.iter().any(
                |response| response.page().is_some_and(|page| page.key() == key)
            ));
            assert!(matches!(
                input.response_custody.front(),
                Some(super::response_custody::RangeResponseCustody::PageNoAliases(page))
                    if page.key() == custody_key
            ));
            let removed = input.response_custody.pop_front().unwrap();
            assert!(removed.page().is_some_and(|page| page.key() == custody_key));
            let PageDemand::Requested(request) = input
                .residency
                .demand(PageRequestId::new(base + 5), purpose, key.demand())
                .unwrap()
            else {
                panic!("settled response must not retain a coalescing reservation")
            };
            let fresh_key = request.key();
            assert_ne!(fresh_key, key);
            assert!(input.dispatched_pages.insert(fresh_key));
            (
                progress,
                ordinary_page_response(request, base + 5, source, malformed),
                fresh_key,
            )
        })
    });
    if malformed {
        assert!(matches!(
            progress,
            super::response_custody::ResponseCustodyProgress::Rejected(RangeTextInputError::Stale)
        ));
    } else {
        assert!(matches!(
            progress,
            super::response_custody::ResponseCustodyProgress::AcceptedTerminal
        ));
    }

    let public = cx.update(|window, app| {
        input.update(app, |input, cx| {
            if !malformed {
                input.residency.force_next_admission_limit();
            }
            input.begin_realization_frame();
            input.deliver_page(fresh_page, window, cx)
        })
    });
    if malformed {
        assert!(matches!(public, Err(RangeTextInputError::Stale)));
    } else {
        assert!(public.is_ok(), "{public:?}");
    }
    input.read_with(cx, |input, _| {
        assert!(!input
            .residency
            .pending_requests()
            .any(|pending| pending == fresh_key));
        assert!(!input.dispatched_pages.contains(&fresh_key));
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleasePage(released) if *released == fresh_key))
                .count(),
            1
        );
        assert!(input
            .residency
            .pending_requests()
            .any(|pending| pending == pending_neighbor));
        assert!(input
            .residency
            .resident_pages()
            .any(|page| page.id() == resident_neighbor));
    });
}

#[gpui::test]
fn ordinary_viewport_and_caret_residency_limit_settle_exact_pending(cx: &mut gpui::TestAppContext) {
    exercise_ordinary_page_admission_failure(cx, PagePurpose::Viewport, false, 950_000);
    exercise_ordinary_page_admission_failure(cx, PagePurpose::Caret, false, 951_000);
}

#[gpui::test]
fn ordinary_viewport_and_caret_malformed_admission_settle_exact_pending(
    cx: &mut gpui::TestAppContext,
) {
    exercise_ordinary_page_admission_failure(cx, PagePurpose::Viewport, true, 952_000);
    exercise_ordinary_page_admission_failure(cx, PagePurpose::Caret, true, 953_000);
}

#[gpui::test]
fn active_coalesced_object_residency_limit_settles_response_and_reissues_current_demand(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(16);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(234),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 16),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 512 * 1024, 8192).unwrap();
    configuration.limits.page_bytes = source.len() as u64;
    configuration.viewport_extent = px(160.);
    configuration.overscan = Pixels::ZERO;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let (key, staged_pending_key, staged_job) =
        stage_active_coalesced_object_response(&input, cx, &source);
    let (job, pending_key, target_intent, index_intent, surface, resident, settled) =
        input.update(cx, |input, _| {
            let pending = input.pending_geometry_object.as_ref().unwrap();
            assert_eq!(pending.request.key(), staged_pending_key);
            assert_eq!(pending.job, staged_job);
            assert_eq!(input.active_geometry, Some(staged_job));
            assert!(matches!(pending.wait, GeometryObjectWait::Coalesced(wait) if wait == key));
            input.object_residency.force_next_admission_limit();
            (
                pending.job,
                pending.request.key(),
                input.pending_target_intent.is_some(),
                input.pending_index_intent,
                fingerprint(input).surface,
                input
                    .object_residency
                    .resident_pages()
                    .map(|page| page.id())
                    .collect::<Vec<_>>(),
                input.superseded_geometry_object_responses_settled,
            )
        });
    let progress = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_geometry_target_object_page_inner(
                empty_terminal_object_response(key),
                true,
                window,
                cx,
            )
        })
    });
    assert!(matches!(
        progress,
        Ok(
            super::response_custody::ResponseDeliveryProgress::AcceptedTerminal(
                RangeTextInputError::SurfaceCapacity
            )
        )
    ));
    input.read_with(cx, |input, _| {
        let pending = input.pending_geometry_object.as_ref().unwrap();
        assert_eq!(input.active_geometry, Some(job));
        assert_eq!(pending.job, job);
        assert_eq!(pending.request.key(), pending_key);
        assert!(matches!(pending.wait, GeometryObjectWait::Coalesced(wait) if wait == key));
        assert_eq!(input.pending_target_intent.is_some(), target_intent);
        assert_eq!(input.pending_index_intent, index_intent);
        assert_eq!(fingerprint(input).surface, surface);
        assert_eq!(
            input
                .object_residency
                .resident_pages()
                .map(|page| page.id())
                .collect::<Vec<_>>(),
            resident
        );
        assert_eq!(
            input.superseded_geometry_object_responses_settled,
            settled
        );
        assert!(!input.dispatched_object_pages.contains(&key));
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                .count(),
            1
        );
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let mut current_requests = 0;
            while let Some(request) = input.take_request() {
                current_requests += usize::from(matches!(
                    request,
                    RangeTextInputRequest::ObjectPage(request) if request.key() == pending_key
                ));
            }
            if current_requests == 0 {
                input.begin_realization_frame();
                input.service_geometry_page(window, cx).unwrap();
                current_requests += usize::from(matches!(
                    input.take_request(),
                    Some(RangeTextInputRequest::ObjectPage(request)) if request.key() == pending_key
                ));
            }
            assert_eq!(current_requests, 1);
            assert_eq!(input.active_geometry, Some(job));
            assert_eq!(
                input
                    .pending_geometry_object
                    .as_ref()
                    .unwrap()
                    .request
                    .key(),
                pending_key
            );
            assert!(input.take_request().is_none());
        })
    });
}

#[gpui::test]
fn active_coalesced_object_full_request_queue_preserves_queue_custody_and_reissues_current_demand(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(16);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(239),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 16),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 512 * 1024, 8192).unwrap();
    configuration.limits.page_bytes = source.len() as u64;
    configuration.viewport_extent = px(160.);
    configuration.overscan = Pixels::ZERO;
    let binding = configuration.binding;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let (key, staged_pending_key, staged_job) =
        stage_active_coalesced_object_response(&input, cx, &source);
    let custody_key = crate::PageRequestKey::adjacent(
        PageRequestId::new(930_000),
        binding.binding(),
        binding.revision(),
        PagePurpose::Viewport,
        ByteOffset::new(0),
        PageDirection::Forward,
        SOURCE.len() as u64,
    )
    .unwrap();
    let custody_page = page_for(PageRequest::new(custody_key), 930_000);
    let (job, pending_key, maximum, preserved, settled) = input.update(cx, |input, _| {
        let pending = input.pending_geometry_object.as_ref().unwrap();
        assert_eq!(pending.request.key(), staged_pending_key);
        assert_eq!(pending.job, staged_job);
        assert_eq!(input.active_geometry, Some(staged_job));
        input
            .response_custody
            .push_back(super::response_custody::RangeResponseCustody::PageNoAliases(custody_page));
        input
            .requests
            .push_back(RangeTextInputRequest::CancelObjectPage(key));
        let mut id = 940_000;
        while input.requests.len() < input.requests.capacity() {
            let filler = crate::PageRequestKey::adjacent(
                PageRequestId::new(id),
                binding.binding(),
                binding.revision(),
                PagePurpose::Viewport,
                ByteOffset::new(0),
                PageDirection::Forward,
                4,
            )
            .unwrap();
            input
                .requests
                .push_back(RangeTextInputRequest::ReleasePage(filler));
            id += 1;
        }
        let preserved = input
            .requests
            .iter()
            .filter(|request| {
                !matches!(request, RangeTextInputRequest::CancelObjectPage(cancelled) if *cancelled == key)
            })
            .map(|request| format!("{request:?}"))
            .collect::<Vec<_>>();
        (
            pending.job,
            pending.request.key(),
            super::super::checked_request_capacity(&input.config).unwrap(),
            preserved,
            input.superseded_geometry_object_responses_settled,
        )
    });
    let progress = cx.update(|window, app| {
        input.update(app, |input, cx| {
            let progress = input.deliver_geometry_target_object_page_inner(
                empty_terminal_object_response(key),
                true,
                window,
                cx,
            );
            let pending = input.pending_geometry_object.as_ref().unwrap();
            assert_eq!(input.active_geometry, Some(job));
            assert_eq!(pending.job, job);
            assert_eq!(pending.request.key(), pending_key);
            assert!(matches!(pending.wait, GeometryObjectWait::Coalesced(wait) if wait == key));
            assert!(!input.dispatched_object_pages.contains(&key));
            assert!(input.requests.len() <= maximum);
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| {
                        !matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key)
                            && !matches!(request, RangeTextInputRequest::ObjectPage(request) if request.key() == pending_key)
                    })
                    .map(|request| format!("{request:?}"))
                    .collect::<Vec<_>>(),
                preserved
            );
            assert!(!input.requests.iter().any(
                |request| matches!(request, RangeTextInputRequest::CancelObjectPage(cancelled) if *cancelled == key)
            ));
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                    .count(),
                1
            );
            assert_eq!(
                input.superseded_geometry_object_responses_settled,
                settled
            );
            assert_eq!(input.response_custody.len(), 1);
            assert!(matches!(
                input.response_custody.front(),
                Some(super::response_custody::RangeResponseCustody::PageNoAliases(page))
                    if page.key() == custody_key
            ));
            progress
        })
    });
    assert!(matches!(
        progress,
        Ok(
            super::response_custody::ResponseDeliveryProgress::AcceptedTerminal(
                RangeTextInputError::SurfaceCapacity
            )
        )
    ));
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let mut current = 0;
            while let Some(request) = input.take_request() {
                current += usize::from(matches!(
                request,
                    RangeTextInputRequest::ObjectPage(request) if request.key() == pending_key
                ));
            }
            if current == 0 {
                input.begin_realization_frame();
                input.service_geometry_page(window, cx).unwrap();
                current += usize::from(matches!(
                    input.take_request(),
                    Some(RangeTextInputRequest::ObjectPage(request)) if request.key() == pending_key
                ));
            }
            assert_eq!(current, 1);
            assert_eq!(input.active_geometry, Some(job));
        })
    });
}

#[gpui::test]
fn public_active_coalesced_object_full_queue_waits_for_capacity_before_exact_reissue(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(16);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(240),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 16),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 512 * 1024, 8192).unwrap();
    configuration.limits.page_bytes = source.len() as u64;
    configuration.limits.max_realization_work_per_frame = 4;
    configuration.viewport_extent = px(160.);
    configuration.overscan = Pixels::ZERO;
    let binding = configuration.binding;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let (key, staged_pending_key, staged_job) =
        stage_active_coalesced_object_response(&input, cx, &source);
    let (job, pending_key, target_intent, index_intent, maximum, preserved, settled) =
        input.update(cx, |input, _| {
            let pending = input.pending_geometry_object.as_ref().unwrap();
            assert_eq!(pending.request.key(), staged_pending_key);
            assert_eq!(pending.job, staged_job);
            assert_eq!(input.active_geometry, Some(staged_job));
            input
                .requests
                .push_back(RangeTextInputRequest::CancelObjectPage(key));
            let mut id = 960_000;
            while input.requests.len() < input.requests.capacity() {
                let filler = crate::PageRequestKey::adjacent(
                    PageRequestId::new(id),
                    binding.binding(),
                    binding.revision(),
                    PagePurpose::Viewport,
                    ByteOffset::new(0),
                    PageDirection::Forward,
                    4,
                )
                .unwrap();
                input
                    .requests
                    .push_back(RangeTextInputRequest::ReleasePage(filler));
                id += 1;
            }
            let preserved = input
                .requests
                .iter()
                .filter(|request| {
                    !matches!(request, RangeTextInputRequest::CancelObjectPage(cancelled) if *cancelled == key)
                })
                .map(|request| format!("{request:?}"))
                .collect::<Vec<_>>();
            assert!(!input.realization_continuation_scheduled);
            (
                pending.job,
                pending.request.key(),
                input.pending_target_intent.is_some(),
                input.pending_index_intent,
                super::super::checked_request_capacity(&input.config).unwrap(),
                preserved,
                input.superseded_geometry_object_responses_settled,
            )
        });

    let result = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            let result = input.deliver_object_page_in_window(
                empty_terminal_object_response(key),
                window,
                cx,
            );
            let pending = input.pending_geometry_object.as_ref().unwrap();
            assert_eq!(input.requests.len(), maximum);
            assert_eq!(input.requests.capacity(), maximum);
            assert_eq!(input.active_geometry, Some(job));
            assert_eq!(pending.job, job);
            assert_eq!(pending.request.key(), pending_key);
            assert!(matches!(pending.wait, GeometryObjectWait::Coalesced(wait) if wait == key));
            assert_eq!(input.pending_target_intent.is_some(), target_intent);
            assert_eq!(input.pending_index_intent, index_intent);
            assert_eq!(
                input.superseded_geometry_object_responses_settled,
                settled
            );
            assert!(!input.dispatched_object_pages.contains(&key));
            assert!(!input.requests.iter().any(
                |request| matches!(request, RangeTextInputRequest::CancelObjectPage(cancelled) if *cancelled == key)
            ));
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                    .count(),
                1
            );
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| {
                        !matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key)
                            && !matches!(request, RangeTextInputRequest::ObjectPage(request) if request.key() == pending_key)
                    })
                    .map(|request| format!("{request:?}"))
                    .collect::<Vec<_>>(),
                preserved
            );
            assert!(!input.requests.iter().any(
                |request| matches!(request, RangeTextInputRequest::ObjectPage(request) if request.key() == pending_key)
            ));
            assert!(input.response_custody.is_empty());
            assert!(!input.realization_continuation_scheduled);
            result
        })
    });
    assert!(result.is_ok(), "{result:?}");

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let drained = input.take_request().unwrap();
            assert!(!matches!(
                drained,
                RangeTextInputRequest::ReleaseObjectPage(released) if released == key
            ));
            assert_eq!(input.requests.len(), maximum - 1);
            input.begin_realization_frame();
            input
                .service_geometry_until_external_boundary(window, cx)
                .unwrap();
            assert_eq!(input.requests.len(), maximum);
            assert_eq!(
                input
                    .requests
                    .iter()
                    .filter(|request| matches!(request, RangeTextInputRequest::ObjectPage(request) if request.key() == pending_key))
                    .count(),
                1
            );
            assert_eq!(input.active_geometry, Some(job));
            assert_eq!(
                input
                    .pending_geometry_object
                    .as_ref()
                    .unwrap()
                    .request
                    .key(),
                pending_key
            );
        })
    });
}

fn detach_terminal_object_response(
    input: &gpui::Entity<RangeTextInput>,
    cx: &mut gpui::VisualTestContext,
    source: &str,
) -> (crate::ObjectRequestKey, crate::GeometryJobKey) {
    let key = stage_terminal_target_object_response(input, cx, source);
    let newer = input.update(cx, |input, _| {
        let old = input
            .pending_geometry_object
            .as_ref()
            .expect("staged object response retains its logical geometry input")
            .job;
        assert!(matches!(
            input.pending_geometry_object.as_ref().unwrap().wait,
            GeometryObjectWait::Coalesced(wait) if wait == key
        ));
        let newer = crate::GeometryJobKey::new(
            old.geometry(),
            crate::GeometryJobId::new(old.job().get().saturating_add(1)),
        );
        input.active_geometry = Some(newer);
        newer
    });
    (key, newer)
}

#[gpui::test]
fn superseded_object_residency_admission_capacity_settles_old_response_only(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(16);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(236),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 16),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 512 * 1024, 8192).unwrap();
    configuration.limits.page_bytes = source.len() as u64;
    configuration.viewport_extent = px(160.);
    configuration.overscan = Pixels::ZERO;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let (key, newer) = detach_terminal_object_response(&input, cx, &source);
    let before = input.update(cx, |input, _| {
        input.object_residency.force_next_admission_limit();
        input.superseded_geometry_object_responses_settled
    });
    let progress = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_geometry_target_object_page_inner(
                empty_terminal_object_response(key),
                true,
                window,
                cx,
            )
        })
    });
    assert!(matches!(
        progress,
        Ok(
            super::response_custody::ResponseDeliveryProgress::AcceptedTerminal(
                RangeTextInputError::SurfaceCapacity
            )
        )
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.active_geometry, Some(newer));
        assert!(input.pending_geometry_object.is_none());
        assert!(!input.dispatched_object_pages.contains(&key));
        assert_eq!(
            input.superseded_geometry_object_responses_settled,
            before + 1
        );
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                .count(),
            1
        );
    });
}

#[gpui::test]
fn superseded_object_full_request_queue_retires_exact_cancel_for_release(
    cx: &mut gpui::TestAppContext,
) {
    let source = "line\n".repeat(16);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(237),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 16),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 512 * 1024, 8192).unwrap();
    configuration.limits.page_bytes = source.len() as u64;
    configuration.viewport_extent = px(160.);
    configuration.overscan = Pixels::ZERO;
    let binding = configuration.binding;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let (key, newer) = detach_terminal_object_response(&input, cx, &source);
    let before = input.update(cx, |input, _| {
        input
            .requests
            .push_back(RangeTextInputRequest::CancelObjectPage(key));
        let mut id = 900_000;
        while input.requests.len() < input.requests.capacity() {
            let filler = crate::PageRequestKey::adjacent(
                PageRequestId::new(id),
                binding.binding(),
                binding.revision(),
                PagePurpose::Viewport,
                ByteOffset::new(0),
                PageDirection::Forward,
                4,
            )
            .unwrap();
            input
                .requests
                .push_back(RangeTextInputRequest::ReleasePage(filler));
            id += 1;
        }
        assert_eq!(input.requests.len(), input.requests.capacity());
        input.superseded_geometry_object_responses_settled
    });
    let progress = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_geometry_target_object_page_inner(
                empty_terminal_object_response(key),
                true,
                window,
                cx,
            )
        })
    });
    assert!(matches!(
        progress,
        Ok(
            super::response_custody::ResponseDeliveryProgress::AcceptedTerminal(
                RangeTextInputError::SurfaceCapacity
            )
        )
    ));
    input.read_with(cx, |input, _| {
        assert_eq!(input.active_geometry, Some(newer));
        assert!(input.pending_geometry_object.is_none());
        assert!(!input.dispatched_object_pages.contains(&key));
        assert_eq!(input.requests.len(), input.requests.capacity());
        assert!(!input.requests.iter().any(
            |request| matches!(request, RangeTextInputRequest::CancelObjectPage(cancelled) if *cancelled == key)
        ));
        assert_eq!(
            input
                .requests
                .iter()
                .filter(|request| matches!(request, RangeTextInputRequest::ReleaseObjectPage(released) if *released == key))
                .count(),
            1
        );
        assert_eq!(
            input.superseded_geometry_object_responses_settled,
            before + 1
        );
    });
}

#[gpui::test]
fn public_rejected_front_preserves_unrelated_object_tail(cx: &mut gpui::TestAppContext) {
    let source = "line\n".repeat(16);
    let mut configuration = config(2 * 1024 * 1024, 32_768);
    configuration.binding = RangeBinding::new(
        BindingId::new(238),
        SourceRevision::new(1),
        LogicalExtent::new(source.len() as u64, 16),
    );
    configuration.geometry_limits =
        ExactGeometryLimits::new(source.len() as u64, 8, 512 * 1024, 8192).unwrap();
    configuration.limits.page_bytes = source.len() as u64;
    configuration.viewport_extent = px(160.);
    configuration.overscan = Pixels::ZERO;
    let binding = configuration.binding;
    let (input, cx) = cx
        .add_window_view(move |window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let object_key = stage_terminal_target_object_response(&input, cx, &source);
    let rejected_key = crate::PageRequestKey::adjacent(
        PageRequestId::new(920_000),
        binding.binding(),
        binding.revision(),
        PagePurpose::Viewport,
        ByteOffset::new(0),
        PageDirection::Forward,
        SOURCE.len() as u64,
    )
    .unwrap();
    let rejected = page_for(PageRequest::new(rejected_key), 920_000);
    let (result, tail_preserved, dispatch_preserved) = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.response_custody.push_back(
                super::response_custody::RangeResponseCustody::ResidentPage(rejected),
            );
            let result = input.deliver_object_page_in_window(
                empty_terminal_object_response(object_key),
                window,
                cx,
            );
            let tail_preserved = input.response_custody.iter().any(|response| {
                matches!(
                    response,
                    super::response_custody::RangeResponseCustody::Object(page)
                        if page.key() == object_key
                )
            });
            (
                result,
                tail_preserved,
                input.dispatched_object_pages.contains(&object_key),
            )
        })
    });
    assert!(matches!(result, Err(RangeTextInputError::Stale)));
    assert!(tail_preserved);
    assert!(dispatch_preserved);
}
