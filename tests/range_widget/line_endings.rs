use super::*;
use gpui::StreamingBoundaryKind;

fn assert_exact_line_endings(cx: &mut gpui::TestAppContext, page_bytes: u64, segment_bytes: usize) {
    let source = "abc\r\nb\rc\nd\r\n";
    let mut configuration = config(source, 1);
    configuration.limits.page_bytes = page_bytes;
    configuration.layout.limits.segment_bytes = segment_bytes;
    let (input, cx) =
        cx.add_window_view(|window, cx| RangeTextInput::new(configuration, window, cx).unwrap());
    assert!(drive_pages(&input, cx, source).is_empty());
    input.read_with(cx, |input, _| {
        assert!(input.is_quiescent());
        assert!(input.is_surface_current_and_interactive());
        assert_eq!(input.realization_diagnostics().response_rejection_count, 0);
        let surface = input.surface().unwrap();
        assert_eq!(surface.binding(), binding(source, 1));
        let delimiters = surface
            .fragments()
            .iter()
            .filter_map(|fragment| {
                let StreamingLayoutFragment::Boundary(boundary) = fragment else {
                    return None;
                };
                let maps = boundary.maps();
                (boundary.kind == StreamingBoundaryKind::LogicalLine && maps.len() == 2).then(
                    || {
                        (
                            maps[0].logical_position.byte_offset,
                            maps[1].logical_position.byte_offset,
                        )
                    },
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(delimiters, [(3, 5), (8, 9), (10, 12)]);
        let text_ranges = surface
            .fragments()
            .iter()
            .filter_map(|fragment| match fragment {
                StreamingLayoutFragment::Text(text) => {
                    let range = text.logical_range();
                    Some((range.start.byte_offset, range.end.byte_offset))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(text_ranges.iter().all(|(start, end)| end - start <= segment_bytes as u64));
        let text = text_ranges
            .iter()
            .map(|(start, end)| &source[*start as usize..*end as usize])
            .collect::<String>();
        assert_eq!(text, "abcb\rcd");
        let before = surface
            .position_for_source_position(ordinary_position(3))
            .unwrap();
        let after = surface
            .position_for_source_position(ordinary_position(5))
            .unwrap();
        assert!(after.y > before.y);
        assert!(
            surface
                .position_for_source_position(ordinary_position(source.len() as u64))
                .is_some()
        );
    });
}

#[gpui::test]
fn exact_source_crlf_uses_two_byte_line_delimiter_and_lone_cr_stays_text(
    cx: &mut gpui::TestAppContext,
) {
    assert_exact_line_endings(cx, 32, 32);
}

#[gpui::test]
fn exact_source_crlf_spanning_pages_preserves_delimiter_and_lone_cr(cx: &mut gpui::TestAppContext) {
    assert_exact_line_endings(cx, 4, 32);
}

#[gpui::test]
fn exact_source_crlf_delimiter_exceeds_text_segment_cap_without_becoming_an_atom(
    cx: &mut gpui::TestAppContext,
) {
    assert_exact_line_endings(cx, 4, 1);
}
