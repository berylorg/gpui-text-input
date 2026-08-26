use super::*;

fn geometry_object_page(input: &mut RangeTextInput, id: u64, purpose: ObjectPurpose) -> ObjectPage {
    let key = crate::ObjectRequestKey::new(
        ObjectRequestId::new(id),
        binding().binding(),
        binding().revision(),
        input.config.presentation_generation,
        purpose,
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

fn sleeping_text_page(input: &mut RangeTextInput, id: u64) -> RangePage {
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

#[gpui::test]
fn windowless_geometry_object_responses_return_exact_payload_without_widget_admission(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    input.update(cx, |input, cx| {
        for (id, purpose) in [
            (620_000, ObjectPurpose::GeometryIndex),
            (620_001, ObjectPurpose::GeometryTarget),
        ] {
            let page = geometry_object_page(input, id, purpose);
            let key = page.key();
            let page_id = page.id();
            let retained = page.retained_charge();
            let before = input.realization_diagnostics().current;
            let RangeTextInputError::ObjectResponseRejected(returned) =
                input.deliver_object_page(page, cx).unwrap_err()
            else {
                panic!("windowless geometry delivery must return typed payload custody")
            };
            assert_eq!(returned.key(), key);
            assert_eq!(returned.id(), page_id);
            assert_eq!(returned.retained_charge(), retained);
            assert_eq!(input.realization_diagnostics().current, before);
            assert!(input.dispatched_object_pages.contains(&key));
            assert!(input.response_custody.is_empty());
            assert!(!input.realization_continuation_scheduled);
        }
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            let _ = input.dispose(window, cx);
        })
    });
}

#[gpui::test]
fn windowless_geometry_rejection_preserves_existing_head_and_windowed_retry_route(
    cx: &mut gpui::TestAppContext,
) {
    let (input, cx) = cx.add_window_view(|window, cx| {
        RangeTextInput::new(config(2 * 1024 * 1024, 32_768), window, cx).unwrap()
    });
    drive_initial_surface(&input, cx);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.begin_realization_frame();
            let sleeping = sleeping_text_page(input, 620_100);
            let sleeping_key = sleeping.key();
            input.deliver_page(sleeping, window, cx).unwrap();
            assert_eq!(input.response_custody.len(), 1);
            assert!(!input.realization_continuation_scheduled);

            let geometry = geometry_object_page(input, 620_101, ObjectPurpose::GeometryTarget);
            let geometry_key = geometry.key();
            let before = input.realization_diagnostics().current;
            let RangeTextInputError::ObjectResponseRejected(geometry) =
                input.deliver_object_page(geometry, cx).unwrap_err()
            else {
                panic!("windowless geometry tail must remain caller-owned")
            };
            assert_eq!(input.realization_diagnostics().current, before);
            assert_eq!(input.response_custody.len(), 1);
            assert!(matches!(
                input.response_custody.front(),
                Some(super::super::response_custody::RangeResponseCustody::Page(page))
                    if page.key() == sleeping_key
            ));
            assert!(input.dispatched_object_pages.contains(&geometry_key));
            assert!(!input.realization_continuation_scheduled);

            let result = input.deliver_object_page_in_window(geometry, window, cx);
            assert!(!matches!(
                result,
                Err(RangeTextInputError::ObjectResponseRejected(_))
            ));
            assert_eq!(input.response_custody.len(), 2);
            assert!(input.response_custody.iter().any(|response| matches!(
                response,
                super::super::response_custody::RangeResponseCustody::Object(page)
                    if page.key() == geometry_key
            )));
            assert!(input.realization_continuation_scheduled);

            let _ = input.dispose(window, cx);
            assert!(input.response_custody.is_empty());
            assert!(!input.realization_continuation_scheduled);
        })
    });
    cx.run_until_parked();
}
