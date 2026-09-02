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
