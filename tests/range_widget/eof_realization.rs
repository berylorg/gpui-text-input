use super::*;

fn assert_eof_caret_realized(
    cx: &mut gpui::TestAppContext,
    trailing_newline: bool,
    checkpoints: usize,
    pending_select_all: bool,
) {
    cx.update(ensure_text_input_bindings);
    let mut source = (0..63)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    source.push_str(if trailing_newline { "end\n" } else { "end" });
    let mut configuration = config(&source, 1);
    configuration.geometry_limits =
        ExactGeometryLimits::new(32, checkpoints, 512 * 1024, 8192).unwrap();
    configuration.viewport_extent = px(640.);
    configuration.limits.max_realized_block_extent = px(64.);
    configuration.overscan = gpui::Pixels::ZERO;
    let (input, cx) = cx.add_window_view(move |window, cx| {
        let input = RangeTextInput::new(configuration, window, cx).unwrap();
        input.focus(window);
        input
    });
    cx.simulate_resize(gpui::size(px(640.), px(640.)));
    if pending_select_all {
        drive_first_local_surface(&input, cx, &source, 10_000);
        while let Some(request) = input.update(cx, |input, _| input.take_request()) {
            assert!(matches!(
                request,
                RangeTextInputRequest::ReleasePage(_) | RangeTextInputRequest::ReleaseObjectPage(_)
            ));
        }
        let RangeTextInputRequest::Page(index) =
            take_request_after_scheduled_frames(&input, cx, "background index")
        else {
            panic!("Select All witness requires a pending index page")
        };
        assert_eq!(index.key().purpose(), PagePurpose::GeometryIndex);
        assert_eq!(
            input.read_with(cx, |input, _| input
                .realization_diagnostics()
                .current
                .checkpoints),
            0,
        );
        cx.simulate_keystrokes("ctrl-a");
        let page = page_for(&source, 1_000_000, index);
        cx.update(|window, app| {
            input.update(app, |input, cx| {
                input.deliver_page(page, window, cx).unwrap()
            })
        });
    } else {
        drive_pages(&input, cx, &source);
        cx.simulate_keystrokes("ctrl-end");
    }
    drive_pages(&input, cx, &source);
    input.read_with(cx, |input, _| {
        assert!(input.is_semantically_quiescent());
        let surface = input.surface().unwrap_or_else(|| {
            panic!("EOF surface missing: {:?}", input.realization_diagnostics())
        });
        let eof = ordinary_position(source.len() as u64);
        let expected = if pending_select_all {
            RangeSourceSelection {
                anchor: ordinary_position(0),
                head: eof,
            }
        } else {
            RangeSourceSelection::caret(eof)
        };
        assert_eq!(surface.selection(), expected);
        let position = surface
            .position_for_source_position(eof)
            .expect("EOF must have exact geometry");
        let caret = surface
            .caret_bounds(px(16.))
            .expect("EOF caret must be realized");
        assert_eq!(caret.origin, position);
        assert_eq!(position.y, px(if trailing_newline { 1024. } else { 1008. }));
        assert!(position.y >= surface.scroll_block());
        let caret_end = position.y + caret.size.height;
        assert!(caret_end <= surface.scroll_block() + px(640.));
        assert!(surface.fillers().all(|filler| {
            caret_end <= filler.block_start() || position.y >= filler.block_end()
        }));
        let diagnostics = input.realization_diagnostics();
        assert!(diagnostics.surface_high_water.bytes <= diagnostics.max_surface_bytes);
        assert!(diagnostics.surface_high_water.items <= diagnostics.max_surface_items);
        assert!(diagnostics.geometry_high_water_bytes <= diagnostics.max_geometry_bytes);
        assert!(diagnostics.geometry_high_water_items <= diagnostics.max_geometry_items);
        assert_eq!(diagnostics.response_rejection_count, 0);
    });
}

#[gpui::test]
fn eof_caret_remains_realized_with_capacity_filler(cx: &mut gpui::TestAppContext) {
    assert_eof_caret_realized(cx, false, 32, false);
}

#[gpui::test]
fn trailing_newline_caret_remains_realized_with_capacity_filler(cx: &mut gpui::TestAppContext) {
    assert_eof_caret_realized(cx, true, 32, false);
}

#[gpui::test]
fn sparse_index_keeps_eof_caret_out_of_capacity_filler(cx: &mut gpui::TestAppContext) {
    assert_eof_caret_realized(cx, false, 2, false);
}

#[gpui::test]
fn sparse_index_keeps_trailing_newline_caret_out_of_capacity_filler(cx: &mut gpui::TestAppContext) {
    assert_eof_caret_realized(cx, true, 2, false);
}

#[gpui::test]
fn pending_select_all_keeps_sparse_trailing_newline_caret_out_of_capacity_filler(
    cx: &mut gpui::TestAppContext,
) {
    assert_eof_caret_realized(cx, true, 2, true);
}
