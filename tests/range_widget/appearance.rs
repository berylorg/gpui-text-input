use super::*;
use gpui::{
    AppContext, Context, Entity, Focusable, IntoElement, Render, Window, canvas, div, hsla,
    prelude::*, rgb,
};

struct RangeAppearanceFixture {
    input: Entity<RangeTextInput>,
    glyph_font_size: gpui::Pixels,
    bounds: gpui::Size<gpui::Pixels>,
    snapshot: Rc<RefCell<Option<gpui::test::PaintSnapshot>>>,
    ambient: gpui::Hsla,
}

impl Render for RangeAppearanceFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let glyph_font_size = self.glyph_font_size;
        let bounds = self.bounds;
        let snapshot = self.snapshot.clone();
        div()
            .text_color(self.ambient)
            .w(bounds.width)
            .h(bounds.height)
            .child(self.input.clone())
            .child(
                canvas(
                    move |_, window, _| {
                        let text =
                            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-[]";
                        let glyphs = window.text_system().shape_line(
                            text.into(),
                            glyph_font_size,
                            &[TextRun {
                                len: text.len(),
                                font: font(".SystemUIFont"),
                                color: black(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            }],
                            None,
                        );
                        window.text_system().set_glyph_raster_bounds_for_test(
                            &glyphs,
                            window.scale_factor(),
                            gpui::Bounds::new(
                                point(gpui::DevicePixels(0), gpui::DevicePixels(-6)),
                                gpui::size(gpui::DevicePixels(4), gpui::DevicePixels(6)),
                            ),
                        );
                    },
                    move |_, _, window, _| {
                        *snapshot.borrow_mut() = Some(window.paint_snapshot_for_test());
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

fn add_range_appearance_fixture(
    cx: &mut gpui::TestAppContext,
    mut configuration: RangeTextInputConfig,
    focus: bool,
) -> (
    Entity<RangeTextInput>,
    &mut gpui::VisualTestContext,
    Rc<RefCell<Option<gpui::test::PaintSnapshot>>>,
    Entity<RangeAppearanceFixture>,
) {
    configuration.limits.max_realized_block_extent = px(256.);
    let snapshot = Rc::new(RefCell::new(None));
    let fixture_snapshot = snapshot.clone();
    let (fixture, cx) = cx.add_window_view(move |window, cx| {
        let glyph_font_size = configuration.layout.font_size;
        let bounds = gpui::size(
            configuration.layout.wrap_width,
            configuration.viewport_extent,
        );
        let input = cx.new(|cx| RangeTextInput::new(configuration, window, cx).unwrap());
        if focus {
            cx.update_entity(&input, |input, _| input.focus(window));
        }
        RangeAppearanceFixture {
            input,
            glyph_font_size,
            bounds,
            snapshot: fixture_snapshot,
            ambient: black(),
        }
    });
    let input = fixture.read_with(cx, |fixture, _| fixture.input.clone());
    (input, cx, snapshot, fixture)
}

fn capture_range_appearance_scene(
    cx: &mut gpui::VisualTestContext,
    snapshot: &Rc<RefCell<Option<gpui::test::PaintSnapshot>>>,
) -> gpui::test::PaintSnapshot {
    *snapshot.borrow_mut() = None;
    cx.update(|window, app| {
        window.refresh();
        window.draw(app).clear();
    });
    snapshot
        .borrow_mut()
        .take()
        .expect("appearance fixture should capture a painted scene")
}

#[gpui::test]
fn range_appearance_repaints_scene_and_preserves_pending_work(cx: &mut gpui::TestAppContext) {
    cx.update(ensure_text_input_bindings);
    let source = (0..100)
        .map(|line| format!("line-{line:03}\n"))
        .collect::<String>();
    let (input, cx, snapshot, fixture) = add_range_appearance_fixture(cx, config(&source, 1), true);
    assert!(drive_pages(&input, cx, &source).is_empty());
    cx.update(|window, app| window.draw(app).clear());
    assert!(drive_pages(&input, cx, &source).is_empty());
    cx.update(|window, app| window.draw(app).clear());
    cx.simulate_keystrokes("shift-right");
    assert!(drive_pages(&input, cx, &source).is_empty());
    cx.update(|window, app| window.draw(app).clear());
    cx.simulate_event(ScrollWheelEvent {
        position: point(px(1.), px(1.)),
        delta: ScrollDelta::Pixels(point(px(0.), px(-48.))),
        ..Default::default()
    });
    let Some(RangeTextInputRequest::Page(pending)) =
        input.update(cx, |input, _| input.take_request())
    else {
        panic!("scroll should leave one dispatched page request")
    };
    let before = range_publication_fingerprint(&input, cx);
    let ownership = input.read_with(cx, |input, _| input.realization_diagnostics().current);
    let focus = input.read_with(cx, |input, app| input.focus_handle(app));
    cx.update(|window, _| assert!(focus.is_focused(window)));
    let events = restoration_events(&input, cx);
    let theme = TextInputTheme {
        text: Some(hsla(0.11, 0.75, 0.52, 1.0)),
        placeholder: hsla(0.21, 0.75, 0.52, 1.0),
        selection: hsla(0.31, 0.75, 0.52, 1.0),
        caret: hsla(0.41, 0.75, 0.52, 1.0),
        marked_underline: hsla(0.51, 0.75, 0.52, 1.0),
        atom_text: hsla(0.61, 0.75, 0.52, 1.0),
        atom_background: Some(hsla(0.71, 0.75, 0.52, 1.0)),
    };
    let scrollbar = ScrollbarStyle {
        thumb_color: 0x2e90fa,
        thickness: px(6.),
        ..ScrollbarStyle::default()
    };
    input.update(cx, |input, cx| {
        input.set_appearance(theme.clone(), scrollbar, cx).unwrap();
    });
    input.read_with(cx, |input, _| {
        assert_eq!(range_publication_fingerprint_from(input), before);
        assert_eq!(input.realization_diagnostics().current, ownership);
        assert_eq!(
            input
                .realization_diagnostics()
                .current
                .dispatched_page_requests,
            1
        );
    });
    assert!(events.borrow().is_empty());
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    assert_eq!(
        input.read_with(cx, |input, app| input.focus_handle(app)),
        focus
    );
    cx.update(|window, _| assert!(focus.is_focused(window)));

    let scene = capture_range_appearance_scene(cx, &snapshot);
    let scale = cx.update(|window, _| window.scale_factor());
    assert!(
        scene
            .glyphs
            .iter()
            .any(|(_, color)| *color == theme.text.unwrap())
    );
    assert!(
        scene
            .backgrounds
            .iter()
            .any(|(_, background)| *background == theme.selection.into())
    );
    assert!(
        scene
            .backgrounds
            .iter()
            .any(|(_, background)| *background == theme.caret.into())
    );
    assert!(
        scene
            .backgrounds
            .iter()
            .any(|(bounds, background)| !background.is_transparent()
                && background.opacity(0.)
                    == gpui::Background::from(rgb(scrollbar.thumb_color)).opacity(0.)
                && bounds.size.width
                    == gpui::ScaledPixels::from(f32::from(scrollbar.thickness) * scale))
    );
    assert_eq!(pending.key().purpose(), PagePurpose::GeometryTarget);
    let prior_scroll = before.surface.scroll_block;
    let page = page_for(&source, pending.key().id().get(), pending);
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.deliver_page(page, window, cx).unwrap()
        })
    });
    assert!(drive_pages(&input, cx, &source).is_empty());
    assert_ne!(
        input.read_with(cx, |input, _| input.surface().unwrap().scroll_block()),
        prior_scroll
    );
    let completed_scene = capture_range_appearance_scene(cx, &snapshot);
    assert!(!completed_scene.glyphs.is_empty());
    assert!(
        completed_scene
            .glyphs
            .iter()
            .all(|(_, color)| *color == theme.text.unwrap())
    );

    let ambient = TextInputTheme {
        text: None,
        ..theme
    };
    input.update(cx, |input, cx| {
        input.set_appearance(ambient, scrollbar, cx).unwrap();
    });
    let scene = capture_range_appearance_scene(cx, &snapshot);
    assert!(scene.glyphs.iter().any(|(_, color)| *color == black()));
    let ambient = hsla(0.83, 0.63, 0.43, 1.0);
    fixture.update(cx, |fixture, cx| {
        fixture.ambient = ambient;
        cx.notify();
    });
    let scene = capture_range_appearance_scene(cx, &snapshot);
    assert!(!scene.glyphs.is_empty());
    assert!(scene.glyphs.iter().all(|(_, color)| *color == ambient));
}

#[gpui::test]
fn range_appearance_repaints_retained_atoms_and_objects(cx: &mut gpui::TestAppContext) {
    let source = format!("xa{}", "\u{301}".repeat(20));
    let mut configuration = config(&source, 1);
    let run = TextRun {
        len: 0,
        font: font(".SystemUIFont"),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    configuration.style = StreamingGeometryStyle::new(
        run.clone(),
        StreamingOversizePresentation::new(
            SharedString::new_static("ATOM"),
            vec![TextRun {
                len: 4,
                ..run.clone()
            }],
            px(32.),
            px(32.),
            px(16.),
            None,
        ),
    );
    let facts = [InlineObjectFact::new(
        InlineObjectId::new(801),
        ByteOffset::new(1),
        InlineObjectOrder::new(1),
        "[object]",
        InlineObjectPresentation::new(
            801,
            SharedString::new_static("OBJECT"),
            px(40.),
            px(32.),
            px(16.),
            None,
            0,
            true,
        )
        .unwrap(),
    )];
    let (input, cx, snapshot, _) = add_range_appearance_fixture(cx, configuration, false);
    cx.update(|window, app| window.draw(app).clear());
    for _ in 0..512 {
        let request = input.update(cx, |input, _| input.take_request());
        match request {
            Some(RangeTextInputRequest::Page(request)) => {
                let base = page_for(&source, request.key().id().get(), request);
                let atom_range = ByteRange::from_u64(0, 1).unwrap();
                let atoms = base
                    .range()
                    .intersection(atom_range)
                    .filter(|range| !range.is_empty())
                    .map(|range| vec![AtomFact::new(AtomId::new(802), atom_range, range, "SOURCE")])
                    .unwrap_or_default();
                let page = RangePage::new(
                    base.id(),
                    request.key(),
                    base.range(),
                    base.text().to_owned(),
                    atoms,
                    base.preceding(),
                    base.following(),
                    base.end_of_source(),
                )
                .unwrap();
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input.deliver_page(page, window, cx).unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ObjectPage(request)) => {
                let page = restoration_object_page(request, &facts, request.key().id().get());
                cx.update(|window, app| {
                    input.update(app, |input, cx| {
                        input
                            .deliver_object_page_in_window(page, window, cx)
                            .unwrap()
                    })
                });
            }
            Some(RangeTextInputRequest::ReleasePage(_))
            | Some(RangeTextInputRequest::CancelPage(_))
            | Some(RangeTextInputRequest::ReleaseObjectPage(_))
            | Some(RangeTextInputRequest::CancelObjectPage(_)) => {}
            Some(request) => panic!("unexpected appearance request: {request:?}"),
            None => {
                cx.update(|window, app| window.draw(app).clear());
                cx.run_until_parked();
                if input.read_with(cx, |input, _| input.is_quiescent()) {
                    break;
                }
            }
        }
    }
    assert!(input.read_with(cx, |input, _| input.is_quiescent()));
    cx.update(|window, app| window.draw(app).clear());
    let fragment_bounds = input.read_with(cx, |input, _| {
        assert!(
            input
                .surface()
                .unwrap()
                .fragments()
                .iter()
                .any(|fragment| { matches!(fragment, StreamingLayoutFragment::OversizeAtom(_)) })
        );
        let fragments = input.surface().unwrap().fragments();
        assert!(fragments.iter().any(|fragment| matches!(fragment, StreamingLayoutFragment::OversizeAtom(atom)
            if atom.logical_range.start.byte_offset == 0 && atom.logical_range.end.byte_offset == 1)));
        assert!(fragments.iter().any(|fragment| matches!(fragment, StreamingLayoutFragment::OversizeAtom(atom)
            if atom.logical_range.start.byte_offset == 1 && atom.logical_range.end.byte_offset == source.len() as u64)));
        assert!(
            input
                .surface()
                .unwrap()
                .fragments()
                .iter()
                .any(|fragment| { matches!(fragment, StreamingLayoutFragment::InlineObject(_)) })
        );
        fragments.iter().filter_map(|fragment| match fragment {
            StreamingLayoutFragment::OversizeAtom(atom) => Some(atom.bounds),
            StreamingLayoutFragment::InlineObject(object) => Some(object.bounds),
            _ => None,
        }).collect::<Vec<_>>()
    });
    let before = range_publication_fingerprint(&input, cx);
    let ownership = input.read_with(cx, |input, _| input.realization_diagnostics().current);
    let theme = TextInputTheme {
        text: Some(hsla(0.09, 0.80, 0.50, 1.0)),
        placeholder: hsla(0.19, 0.80, 0.50, 1.0),
        selection: hsla(0.29, 0.80, 0.50, 1.0),
        caret: hsla(0.39, 0.80, 0.50, 1.0),
        marked_underline: hsla(0.49, 0.80, 0.50, 1.0),
        atom_text: hsla(0.59, 0.80, 0.50, 1.0),
        atom_background: Some(hsla(0.69, 0.80, 0.50, 1.0)),
    };
    input.update(cx, |input, cx| {
        input
            .set_appearance(theme.clone(), ScrollbarStyle::default(), cx)
            .unwrap();
    });
    input.read_with(cx, |input, _| {
        assert_eq!(range_publication_fingerprint_from(input), before);
        assert_eq!(input.realization_diagnostics().current, ownership);
    });
    let scene = capture_range_appearance_scene(cx, &snapshot);
    let scale = cx.update(|window, _| window.scale_factor());
    assert_eq!(fragment_bounds.len(), 3);
    for bounds in &fragment_bounds {
        let bounds = bounds.scale(scale);
        assert!(
            scene
                .glyphs
                .iter()
                .any(|(glyph, color)| *color == theme.atom_text && bounds.contains(&glyph.origin)),
            "missing glyph inside {bounds:?}"
        );
        assert!(
            scene
                .backgrounds
                .iter()
                .any(|(quad, background)| *quad == bounds
                    && *background == theme.atom_background.unwrap().into()),
            "missing background at {bounds:?}"
        );
    }
    input.update(cx, |input, cx| {
        input
            .set_appearance(
                TextInputTheme {
                    atom_background: None,
                    ..theme.clone()
                },
                ScrollbarStyle::default(),
                cx,
            )
            .unwrap()
    });
    let scene = capture_range_appearance_scene(cx, &snapshot);
    for bounds in fragment_bounds {
        let bounds = bounds.scale(scale);
        assert!(!scene.backgrounds.iter().any(|(quad, _)| *quad == bounds));
        assert!(
            scene
                .glyphs
                .iter()
                .any(|(glyph, color)| *color == theme.atom_text && bounds.contains(&glyph.origin))
        );
    }
}

#[gpui::test]
fn range_appearance_repaints_placeholder_scene(cx: &mut gpui::TestAppContext) {
    let mut configuration = config("", 1);
    configuration.placeholder = SharedString::new_static("placeholder");
    let (input, cx, snapshot, _) = add_range_appearance_fixture(cx, configuration, false);
    assert!(drive_pages(&input, cx, "").is_empty());
    let theme = TextInputTheme {
        text: Some(hsla(0.15, 0.70, 0.50, 1.0)),
        placeholder: hsla(0.25, 0.70, 0.50, 1.0),
        selection: hsla(0.35, 0.70, 0.50, 1.0),
        caret: hsla(0.45, 0.70, 0.50, 1.0),
        marked_underline: hsla(0.55, 0.70, 0.50, 1.0),
        atom_text: hsla(0.65, 0.70, 0.50, 1.0),
        atom_background: Some(hsla(0.75, 0.70, 0.50, 1.0)),
    };
    input.update(cx, |input, cx| {
        input
            .set_appearance(theme.clone(), ScrollbarStyle::default(), cx)
            .unwrap();
    });
    let scene = capture_range_appearance_scene(cx, &snapshot);
    assert!(
        scene
            .glyphs
            .iter()
            .any(|(_, color)| *color == theme.placeholder),
        "placeholder snapshot: {scene:?}"
    );
}

#[gpui::test]
fn range_appearance_repaints_marked_underline_scene(cx: &mut gpui::TestAppContext) {
    let source = "x";
    let successor = "markedx";
    let (input, cx, snapshot, _) = add_range_appearance_fixture(cx, config(source, 1), true);
    cx.update(|window, app| window.draw(app).clear());
    assert!(drive_pages(&input, cx, source).is_empty());
    cx.update(|window, app| window.draw(app).clear());
    assert!(input.read_with(cx, |input, _| input.is_quiescent()));
    input.update(cx, |input, _| {
        admit_ordinary_edit_positions(input, source, 1, &[0])
    });
    let (key, positions) = cx.update(|window, app| {
        input.update(app, |input, cx| {
            input.replace_and_mark_text_in_range(None, "marked", None, window, cx);
            let request = input.take_request();
            let Some(RangeTextInputRequest::MutationBegin(begin)) = request else {
                panic!("marked replacement should begin a mutation; got {request:?}")
            };
            let key = begin.proposal().key();
            input.accept_mutation_preflight(key, cx).unwrap();
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::MutationProposalPage(_))
            ));
            let Some(RangeTextInputRequest::MutationFinishInput(finish)) = input.take_request()
            else {
                panic!("marked replacement should finish mutation input")
            };
            let positions = finish.intended();
            input.accept_mutation_finish(key, cx).unwrap();
            assert!(matches!(
                input.take_request(),
                Some(RangeTextInputRequest::MutationCommit(request)) if request.key() == key
            ));
            (key, positions)
        })
    });
    let (text, objects) = admitted_sources(
        successor,
        2,
        &[
            positions.caret(),
            positions.selection_anchor(),
            positions.selection_head(),
        ],
    );
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            input
                .settle_committed_mutation(
                    key,
                    binding(successor, 2),
                    positions,
                    &text,
                    &objects,
                    window,
                    cx,
                )
                .unwrap();
        })
    });
    assert!(drive_pages(&input, cx, successor).is_empty());
    input.read_with(cx, |input, _| {
        assert_eq!(
            input.surface().unwrap().composition(),
            Some(ByteRange::from_u64(0, 6).unwrap())
        );
    });
    let before = range_publication_fingerprint(&input, cx);
    let ownership = input.read_with(cx, |input, _| input.realization_diagnostics().current);
    let events = restoration_events(&input, cx);
    let theme = TextInputTheme {
        text: Some(hsla(0.17, 0.70, 0.50, 1.0)),
        placeholder: hsla(0.27, 0.70, 0.50, 1.0),
        selection: hsla(0.37, 0.70, 0.50, 1.0),
        caret: hsla(0.47, 0.70, 0.50, 1.0),
        marked_underline: hsla(0.57, 0.70, 0.50, 1.0),
        atom_text: hsla(0.67, 0.70, 0.50, 1.0),
        atom_background: Some(hsla(0.77, 0.70, 0.50, 1.0)),
    };
    input.update(cx, |input, cx| {
        input
            .set_appearance(theme.clone(), ScrollbarStyle::default(), cx)
            .unwrap();
    });
    input.read_with(cx, |input, _| {
        assert_eq!(range_publication_fingerprint_from(input), before);
        assert_eq!(input.realization_diagnostics().current, ownership);
        assert_eq!(
            input.surface().unwrap().composition(),
            Some(ByteRange::from_u64(0, 6).unwrap())
        );
    });
    assert!(events.borrow().is_empty());
    assert!(input.update(cx, |input, _| input.take_request()).is_none());
    let scene = capture_range_appearance_scene(cx, &snapshot);
    assert!(
        scene
            .backgrounds
            .iter()
            .any(|(_, background)| *background == theme.marked_underline.into())
    );
}
