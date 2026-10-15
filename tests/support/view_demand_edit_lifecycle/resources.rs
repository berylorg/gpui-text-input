use super::*;

fn delayed_object(
    cx: &mut gpui::TestAppContext,
    probe: AdmissionProbe,
) -> crate::RangeSurfaceCharge {
    let source = "x".repeat(192);
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(long_config(), window, cx).unwrap());
    let Some(RangeTextInputRequest::Page(request)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("original text demand")
    };
    let edit = input.update(cx, begin_local_edit);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_page(page_for_source(request, 1, &source), window, cx)
                .unwrap();
            let logical = logical_object(input.pending_geometry_object.as_ref().unwrap());
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::MutationBegin(_))
            ));
            input.reject_mutation_preflight(edit, cx).unwrap();
            input.begin_realization_frame();
            let original = crate::RangeSurfaceCharge {
                bytes: input.config.limits.max_surface_bytes,
                items: input.config.limits.max_surface_items,
            };
            let original_id = input.next_id;
            input.realization_high_water.owned_bytes = 0;
            input.realization_high_water.owned_items = 0;
            match probe {
                AdmissionProbe::Capacity { limit, .. } => {
                    input.config.limits.max_surface_bytes = limit.bytes;
                    input.config.limits.max_surface_items = limit.items;
                }
                AdmissionProbe::Exhaustion => input.next_id = u64::MAX,
                AdmissionProbe::Ordinary => {}
            }
            let before = fingerprint(input);
            let result = input.service_geometry_until_external_boundary(window, cx);
            if matches!(
                probe,
                AdmissionProbe::Capacity { refuse: true, .. } | AdmissionProbe::Exhaustion
            ) {
                if matches!(probe, AdmissionProbe::Exhaustion) {
                    assert!(matches!(result, Err(RangeTextInputError::Stale)));
                } else {
                    assert!(matches!(result, Err(RangeTextInputError::SurfaceCapacity)));
                }
                assert_eq!(fingerprint(input), before);
                assert_eq!(
                    logical_object(input.pending_geometry_object.as_ref().unwrap()),
                    logical
                );
                assert_eq!(input.object_residency.pending_requests().len(), 0);
                input.config.limits.max_surface_bytes = original.bytes;
                input.config.limits.max_surface_items = original.items;
                input.next_id = original_id;
                input.begin_realization_frame();
                input
                    .service_geometry_until_external_boundary(window, cx)
                    .unwrap();
            } else {
                result.unwrap();
            }
            let request = input
                .requests
                .iter()
                .find_map(|request| match request {
                    RangeTextInputRequest::ObjectPage(request) => Some(*request),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                (request.key().purpose(), request.key().demand()),
                (logical.1, logical.2)
            );
            assert_eq!(input.active_geometry, Some(logical.0));
            assert!(
                input
                    .object_residency
                    .pending_requests()
                    .any(|key| key == request.key())
            );
            let diagnostics = input.realization_diagnostics();
            assert!(
                diagnostics.current.pending_object_bytes
                    >= request.key().demand().max_retained_bytes()
            );
            assert!(diagnostics.current.owned_bytes <= input.config.limits.max_surface_bytes);
            assert!(diagnostics.current.owned_items <= input.config.limits.max_surface_items);
            assert!(diagnostics.current.owned_bytes <= diagnostics.high_water.owned_bytes);
            assert!(diagnostics.current.owned_items <= diagnostics.high_water.owned_items);
            crate::RangeSurfaceCharge {
                bytes: diagnostics.high_water.owned_bytes,
                items: diagnostics.high_water.owned_items,
            }
        })
    })
}

#[gpui::test]
fn competing_clipboard_delayed_text_admission_obeys_byte_peak_exact_fit_and_one_under(
    cx: &mut gpui::TestAppContext,
) {
    let peak = copy_with_delayed_text(cx, AdmissionProbe::Ordinary);
    assert!(peak.bytes > 1);
    copy_with_delayed_text(
        cx,
        AdmissionProbe::Capacity {
            limit: crate::RangeSurfaceCharge {
                bytes: peak.bytes - 1,
                items: 32_768,
            },
            refuse: true,
        },
    );
    let exact = copy_with_delayed_text(
        cx,
        AdmissionProbe::Capacity {
            limit: crate::RangeSurfaceCharge {
                bytes: peak.bytes,
                items: 32_768,
            },
            refuse: false,
        },
    );
    assert_eq!(exact.bytes, peak.bytes);
}

#[gpui::test]
fn competing_clipboard_delayed_text_admission_obeys_item_peak_exact_fit_and_one_under(
    cx: &mut gpui::TestAppContext,
) {
    let peak = copy_with_delayed_text(cx, AdmissionProbe::Ordinary);
    assert!(peak.items > 1);
    copy_with_delayed_text(
        cx,
        AdmissionProbe::Capacity {
            limit: crate::RangeSurfaceCharge {
                bytes: 2 * 1024 * 1024,
                items: peak.items - 1,
            },
            refuse: true,
        },
    );
    let exact = copy_with_delayed_text(
        cx,
        AdmissionProbe::Capacity {
            limit: crate::RangeSurfaceCharge {
                bytes: 2 * 1024 * 1024,
                items: peak.items,
            },
            refuse: false,
        },
    );
    assert_eq!(exact.items, peak.items);
}

#[gpui::test]
fn competing_clipboard_geometry_id_exhaustion_retains_original_intent_and_admitted_custody(
    cx: &mut gpui::TestAppContext,
) {
    copy_with_delayed_text(cx, AdmissionProbe::Exhaustion);
}

#[gpui::test]
fn delayed_object_admission_reserves_payload_with_exact_byte_peak_and_one_under(
    cx: &mut gpui::TestAppContext,
) {
    let peak = delayed_object(cx, AdmissionProbe::Ordinary);
    delayed_object(
        cx,
        AdmissionProbe::Capacity {
            limit: crate::RangeSurfaceCharge {
                bytes: peak.bytes - 1,
                items: 32_768,
            },
            refuse: true,
        },
    );
    let exact = delayed_object(
        cx,
        AdmissionProbe::Capacity {
            limit: crate::RangeSurfaceCharge {
                bytes: peak.bytes,
                items: 32_768,
            },
            refuse: false,
        },
    );
    assert_eq!(exact.bytes, peak.bytes);
}

#[gpui::test]
fn delayed_object_admission_preserves_exact_item_peak_and_one_under(cx: &mut gpui::TestAppContext) {
    let peak = delayed_object(cx, AdmissionProbe::Ordinary);
    delayed_object(
        cx,
        AdmissionProbe::Capacity {
            limit: crate::RangeSurfaceCharge {
                bytes: 2 * 1024 * 1024,
                items: peak.items - 1,
            },
            refuse: true,
        },
    );
    let exact = delayed_object(
        cx,
        AdmissionProbe::Capacity {
            limit: crate::RangeSurfaceCharge {
                bytes: 2 * 1024 * 1024,
                items: peak.items,
            },
            refuse: false,
        },
    );
    assert_eq!(exact.items, peak.items);
}

#[gpui::test]
fn delayed_object_id_exhaustion_retains_logical_job_without_transport_custody(
    cx: &mut gpui::TestAppContext,
) {
    delayed_object(cx, AdmissionProbe::Exhaustion);
}

#[gpui::test]
fn completed_index_defers_its_new_target_without_reserving_transport_identity(
    cx: &mut gpui::TestAppContext,
) {
    let source = "x".repeat(1024);
    let mut configuration = long_config();
    configuration.binding = RangeBinding::new(
        configuration.binding.binding(),
        configuration.binding.revision(),
        LogicalExtent::new(source.len() as u64, 1),
    );
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    let final_objects = (1..1024).find_map(|id| {
        cx.update(|window, app| window.draw(app).clear());
        cx.run_until_parked();
        match input.update(cx, |input, _| input.take_request()) {
            Some(RangeTextInputRequest::Page(request)) => { cx.update(|window, app| input.update(app, |input, cx| input.deliver_page(page_for_source(request, id, &source), window, cx).unwrap())); None }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                if request.key().purpose() == ObjectPurpose::GeometryIndex && matches!(request.key().demand(), ObjectDemandEnvelope::Range { range, .. } if range.end().get() == source.len() as u64) { Some(request) }
                else { cx.update(|window, app| input.update(app, |input, cx| input.deliver_object_page_in_window(empty_objects(request, id), window, cx).unwrap())); None }
            }
            Some(RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_) | RangeTextInputRequest::CancelPage(_) | RangeTextInputRequest::CancelObjectPage(_)) | None => None,
            other => panic!("original indexing request: {other:?}"),
        }
    }).expect("original final index object response");
    let index_job = input.read_with(cx, |input, _| input.active_geometry.unwrap());
    let edit = input.update(cx, begin_local_edit);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .deliver_object_page_in_window(empty_objects(final_objects, 40_000), window, cx)
                .unwrap();
            assert!(input.geometry.index().is_some());
            let logical = logical_text(input.pending_geometry_page.as_ref().unwrap());
            assert_ne!(logical.0, index_job);
            assert_eq!(input.active_geometry, Some(logical.0));
            assert_eq!(input.residency.pending_requests().len(), 0);
            let next_id = input.next_id;
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::MutationBegin(_))
            ));
            input.reject_mutation_preflight(edit, cx).unwrap();
            input.begin_realization_frame();
            input
                .service_geometry_until_external_boundary(window, cx)
                .unwrap();
            assert!(input.next_id > next_id);
            assert_eq!(
                input.config.binding.binding(),
                logical.0.geometry().binding()
            );
            assert_eq!(
                input.config.binding.revision(),
                logical.0.geometry().revision()
            );
            assert!(
                input
                    .pending_geometry_page
                    .as_ref()
                    .is_none_or(|pending| pending.request.is_some())
            );
        })
    });
}
