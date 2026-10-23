//! The editor driven by input events, as the platform drives it: the scenario of SPEC §3.3.

use super::*;

/// A 400 × 300 image in a 960 × 600 window at 100%: it is shown at (280, 172), 1 : 1.
const IMAGE_AT: (f32, f32) = (280.0, 172.0);

fn image() -> Pixmap {
    let mut p = Pixmap::new(400, 300).unwrap();
    for (i, c) in p.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i % 400, i / 400);
        *c = [(x / 2) as u8, (y / 2) as u8, 180, 255];
    }
    p
}

fn open(destination: Destination, has_file: bool) -> Editor {
    Editor::new(
        Open {
            image: image(),
            image_scale: 1.0,
            name: "Shot 2026-10-22 17-03-15.png".into(),
            has_file,
            window: (960, 600),
            scale: 1.0,
            dark: false,
            destination,
            os: Os::Windows,
        },
        Instant::now(),
    )
}

fn editor() -> Editor {
    open(Destination::Both, true)
}

/// Window px of image point (`x`, `y`).
fn at(x: f32, y: f32) -> (f32, f32) {
    (IMAGE_AT.0 + x, IMAGE_AT.1 + y)
}

fn now(e: &Editor) -> Instant {
    e.now
}

fn drag(e: &mut Editor, from: (f32, f32), to: (f32, f32)) {
    let t = now(e);
    let shift = false;
    e.handle(
        Input::Move {
            x: from.0,
            y: from.1,
            shift,
        },
        t,
    );
    e.handle(
        Input::Down {
            x: from.0,
            y: from.1,
            shift,
        },
        t,
    );
    let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    e.handle(
        Input::Move {
            x: mid.0,
            y: mid.1,
            shift,
        },
        t,
    );
    e.handle(
        Input::Move {
            x: to.0,
            y: to.1,
            shift,
        },
        t,
    );
    e.handle(
        Input::Up {
            x: to.0,
            y: to.1,
            shift,
        },
        t,
    );
}

fn click(e: &mut Editor, p: (f32, f32)) {
    drag(e, p, p);
}

fn key(e: &mut Editor, key: Key, ctrl: bool, shift: bool) {
    let t = now(e);
    e.handle(Input::Key { key, ctrl, shift }, t);
}

fn char(e: &mut Editor, c: char) {
    key(e, Key::Char(c), false, false);
}

fn ctrl(e: &mut Editor, c: char) {
    key(e, Key::Char(c), true, false);
}

/// Window px of the middle of a toolbar control.
fn button(e: &Editor, b: Button) -> (f32, f32) {
    let r = e.toolbar.button_rect(b);
    ((r.x + r.width / 2) as f32, (r.y + r.height / 2) as f32)
}

#[test]
fn draws_an_arrow_and_undoes_it() {
    let mut e = editor();
    assert_eq!(e.tool(), Tool::Arrow);
    assert!(!e.dirty());
    assert_eq!(e.title(), "Shot 2026-10-22 17-03-15.png");

    drag(&mut e, at(20.0, 30.0), at(120.0, 80.0));
    assert_eq!(e.objects().len(), 1);
    assert_eq!(
        e.objects()[0].shape,
        Shape::Arrow {
            a: Pt::new(20.0, 30.0),
            b: Pt::new(120.0, 80.0)
        }
    );
    assert_eq!((e.objects()[0].color, e.objects()[0].width), (0, Width::M));
    assert_eq!(e.selected(), Some(0));
    assert!(e.dirty());
    assert_eq!(e.title(), "* Shot 2026-10-22 17-03-15.png");

    ctrl(&mut e, 'z');
    assert!(e.objects().is_empty());
    assert!(!e.dirty());
    key(&mut e, Key::Char('z'), true, true);
    assert_eq!(e.objects().len(), 1);
    ctrl(&mut e, 'z');
    ctrl(&mut e, 'y');
    assert_eq!(e.objects().len(), 1);
    ctrl(&mut e, 'y');
    assert_eq!(e.objects().len(), 1, "nothing left to redo");
}

#[test]
fn a_click_draws_nothing_and_drawing_stays_on_the_image() {
    let mut e = editor();
    click(&mut e, at(50.0, 50.0));
    assert!(e.objects().is_empty());
    assert!(!e.history.can_undo());

    char(&mut e, 'r');
    drag(&mut e, at(350.0, 250.0), at(600.0, 500.0));
    assert_eq!(
        e.objects()[0].shape,
        Shape::Rect {
            a: Pt::new(350.0, 250.0),
            b: Pt::new(400.0, 300.0)
        }
    );
}

#[test]
fn shift_squares_a_rect() {
    let mut e = editor();
    char(&mut e, 'r');
    let t = now(&e);
    let (x0, y0) = at(10.0, 10.0);
    let (x1, y1) = at(110.0, 60.0);
    e.handle(
        Input::Down {
            x: x0,
            y: y0,
            shift: false,
        },
        t,
    );
    e.handle(
        Input::Move {
            x: x1,
            y: y1,
            shift: true,
        },
        t,
    );
    e.handle(
        Input::Up {
            x: x1,
            y: y1,
            shift: true,
        },
        t,
    );
    let Shape::Rect { a, b } = e.objects()[0].shape else {
        panic!()
    };
    assert_eq!((b.x - a.x).abs(), (b.y - a.y).abs());
}

#[test]
fn select_move_resize_nudge_and_delete() {
    let mut e = editor();
    char(&mut e, 'r');
    drag(&mut e, at(100.0, 100.0), at(200.0, 160.0));
    char(&mut e, 'v');
    assert_eq!(e.tool(), Tool::Select);

    // Off the rect, a click lets the selection go.
    click(&mut e, at(300.0, 250.0));
    assert_eq!(e.selected(), None);
    assert_eq!(e.cursor(), Cursor::Arrow);

    // Its outline takes it: a drag there moves it.
    e.handle(
        Input::Move {
            x: at(100.0, 130.0).0,
            y: at(100.0, 130.0).1,
            shift: false,
        },
        now(&e),
    );
    assert_eq!(e.cursor(), Cursor::Move);
    drag(&mut e, at(100.0, 130.0), at(105.0, 135.0));
    assert_eq!(e.selected(), Some(0));

    // So does its inside.
    click(&mut e, at(300.0, 250.0));
    e.handle(
        Input::Move {
            x: at(155.0, 140.0).0,
            y: at(155.0, 140.0).1,
            shift: false,
        },
        now(&e),
    );
    assert_eq!(e.cursor(), Cursor::Move);
    drag(&mut e, at(155.0, 140.0), at(160.0, 145.0));
    assert_eq!(e.selected(), Some(0));
    assert_eq!(
        e.objects()[0].shape,
        Shape::Rect {
            a: Pt::new(110.0, 110.0),
            b: Pt::new(210.0, 170.0)
        }
    );

    // The bottom-right handle resizes.
    e.handle(
        Input::Move {
            x: at(210.0, 170.0).0,
            y: at(210.0, 170.0).1,
            shift: false,
        },
        now(&e),
    );
    assert_eq!(e.cursor(), Cursor::Resize(Axis::Falling));
    drag(&mut e, at(210.0, 170.0), at(250.0, 200.0));
    assert_eq!(
        e.objects()[0].shape,
        Shape::Rect {
            a: Pt::new(110.0, 110.0),
            b: Pt::new(250.0, 200.0)
        }
    );

    // Three nudges, one step back.
    key(&mut e, Key::Right, false, false);
    key(&mut e, Key::Right, false, false);
    key(&mut e, Key::Down, false, true);
    assert_eq!(
        e.objects()[0].shape,
        Shape::Rect {
            a: Pt::new(112.0, 120.0),
            b: Pt::new(252.0, 210.0)
        }
    );
    ctrl(&mut e, 'z');
    assert_eq!(
        e.objects()[0].shape,
        Shape::Rect {
            a: Pt::new(110.0, 110.0),
            b: Pt::new(250.0, 200.0)
        }
    );

    key(&mut e, Key::Delete, false, false);
    assert!(e.objects().is_empty());
    ctrl(&mut e, 'z');
    assert_eq!(e.objects().len(), 1);
    assert_eq!(e.selected(), Some(0));
}

#[test]
fn inside_a_shape_what_lies_in_it_wins() {
    let mut e = editor();
    drag(&mut e, at(100.0, 150.0), at(200.0, 150.0));
    char(&mut e, 'r');
    drag(&mut e, at(250.0, 50.0), at(300.0, 100.0));
    drag(&mut e, at(20.0, 20.0), at(380.0, 280.0));
    char(&mut e, 'v');

    // The arrow under the big rect, the small rect in it, the big rect's own inside.
    click(&mut e, at(150.0, 150.0));
    assert_eq!(e.selected(), Some(0));
    click(&mut e, at(275.0, 75.0));
    assert_eq!(e.selected(), Some(1));
    click(&mut e, at(60.0, 250.0));
    assert_eq!(e.selected(), Some(2));
}

#[test]
fn escape_drops_a_drag_in_progress() {
    let mut e = editor();
    let t = now(&e);
    let (x, y) = at(10.0, 10.0);
    e.handle(Input::Down { x, y, shift: false }, t);
    e.handle(
        Input::Move {
            x: x + 50.0,
            y: y + 50.0,
            shift: false,
        },
        t,
    );
    assert_eq!(e.objects().len(), 1);
    key(&mut e, Key::Escape, false, false);
    assert!(e.objects().is_empty());
    e.handle(
        Input::Up {
            x: x + 50.0,
            y: y + 50.0,
            shift: false,
        },
        t,
    );
    assert!(e.objects().is_empty());
    assert!(e.take_requests().is_empty(), "the Esc did not close");
}

#[test]
fn moving_keeps_the_object_on_the_image() {
    let mut e = editor();
    char(&mut e, 'l');
    drag(&mut e, at(10.0, 10.0), at(50.0, 10.0));
    char(&mut e, 'v');
    drag(&mut e, at(30.0, 10.0), (0.0, 0.0));
    let Shape::Line { a, b } = e.objects()[0].shape else {
        panic!()
    };
    // The middle rests on the image's corner.
    assert_eq!(
        Pt::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0),
        Pt::new(0.0, 0.0)
    );
}

#[test]
fn counters_number_on_and_n_switches_to_letters() {
    let mut e = editor();
    char(&mut e, 'n');
    assert_eq!(e.tool(), Tool::Counter(Counting::Numbers));
    click(&mut e, at(50.0, 50.0));
    click(&mut e, at(150.0, 50.0));
    let ns: Vec<u32> = e
        .objects()
        .iter()
        .filter_map(|o| match o.shape {
            Shape::Counter { n, .. } => Some(n),
            _ => None,
        })
        .collect();
    assert_eq!(ns, [1, 2]);
    // One undo per counter.
    ctrl(&mut e, 'z');
    assert_eq!(e.objects().len(), 1);

    char(&mut e, 'n');
    assert_eq!(e.tool(), Tool::Counter(Counting::Letters));
    // A drag places a counter where it ends.
    drag(&mut e, at(200.0, 200.0), at(220.0, 210.0));
    assert_eq!(
        e.objects()[1].shape,
        Shape::Counter {
            at: Pt::new(220.0, 210.0),
            counting: Counting::Letters,
            n: 1
        }
    );
    // Another tool, then N: the counting last used.
    char(&mut e, 'a');
    char(&mut e, 'n');
    assert_eq!(e.tool(), Tool::Counter(Counting::Letters));
}

#[test]
fn toolbar_picks_tools_colors_and_widths() {
    let mut e = editor();
    let rect = button(&e, Button::Tool(Tool::Rect));
    e.handle(
        Input::Move {
            x: rect.0,
            y: rect.1,
            shift: false,
        },
        now(&e),
    );
    assert_eq!(e.cursor(), Cursor::Arrow);
    click(&mut e, rect);
    assert_eq!(e.tool(), Tool::Rect);

    // A press that leaves the button does nothing.
    let t = now(&e);
    let ellipse = button(&e, Button::Tool(Tool::Ellipse));
    e.handle(
        Input::Down {
            x: ellipse.0,
            y: ellipse.1,
            shift: false,
        },
        t,
    );
    e.handle(
        Input::Up {
            x: 5.0,
            y: 300.0,
            shift: false,
        },
        t,
    );
    assert_eq!(e.tool(), Tool::Rect);

    drag(&mut e, at(10.0, 10.0), at(100.0, 100.0));
    // With an object selected, a color is its color, as one step.
    let b = button(&e, Button::Color(3));
    click(&mut e, b);
    let b = button(&e, Button::Width(Width::L));
    click(&mut e, b);
    assert_eq!((e.objects()[0].color, e.objects()[0].width), (3, Width::L));
    ctrl(&mut e, 'z');
    assert_eq!((e.objects()[0].color, e.objects()[0].width), (3, Width::M));
    // And the next object takes them too.
    drag(&mut e, at(150.0, 150.0), at(200.0, 200.0));
    assert_eq!((e.objects()[1].color, e.objects()[1].width), (3, Width::L));

    // Undo and redo on the toolbar; a disabled one does nothing.
    let b = button(&e, Button::Undo);
    click(&mut e, b);
    assert_eq!(e.objects().len(), 1);
    let b = button(&e, Button::Redo);
    click(&mut e, b);
    assert_eq!(e.objects().len(), 2);
    let b = button(&e, Button::Redo);
    click(&mut e, b);
    assert_eq!(e.objects().len(), 2);
}

#[test]
fn closing_asks_only_with_unsaved_edits() {
    let mut e = editor();
    key(&mut e, Key::Escape, false, false);
    assert_eq!(e.take_requests(), [Request::Close]);

    let mut e = editor();
    drag(&mut e, at(10.0, 10.0), at(100.0, 100.0));
    key(&mut e, Key::Escape, false, false);
    assert!(e.take_requests().is_empty());
    assert!(e.dialog.is_some());
    // The dialog holds the keys and the mouse.
    char(&mut e, 'r');
    assert_eq!(e.tool(), Tool::Arrow);
    key(&mut e, Key::Escape, false, false);
    assert!(e.dialog.is_none());
    assert_eq!(e.objects().len(), 1);

    let t = now(&e);
    e.handle(Input::Close, t);
    let discard = e.dialog.as_ref().unwrap().layout.buttons[0].1;
    click(&mut e, ((discard.x + 4) as f32, (discard.y + 4) as f32));
    assert_eq!(e.take_requests(), [Request::Close]);

    e.handle(Input::Close, t);
    key(&mut e, Key::Enter, false, false);
    assert_eq!(
        e.take_requests(),
        [Request::SaveAndClose {
            file: true,
            copy: true
        }]
    );
}

#[test]
fn enter_finishes_by_the_destination() {
    for (destination, has_file, edit, want) in [
        (Destination::Both, true, true, (true, true)),
        // Nothing new to write: the file already holds it.
        (Destination::Both, true, false, (false, true)),
        (Destination::Clipboard, false, true, (false, true)),
        (Destination::File, false, false, (true, false)),
    ] {
        let mut e = open(destination, has_file);
        if edit {
            drag(&mut e, at(10.0, 10.0), at(100.0, 100.0));
        }
        key(&mut e, Key::Enter, false, false);
        assert_eq!(
            e.take_requests(),
            [Request::SaveAndClose {
                file: want.0,
                copy: want.1
            }],
            "{destination:?} {has_file} {edit}"
        );
    }
}

#[test]
fn saving_marks_the_edits_saved() {
    let mut e = editor();
    drag(&mut e, at(10.0, 10.0), at(100.0, 100.0));
    ctrl(&mut e, 's');
    assert_eq!(e.take_requests(), [Request::Save]);
    // An edit while the file is written stays unsaved.
    drag(&mut e, at(150.0, 150.0), at(200.0, 200.0));
    e.saved(None, now(&e));
    assert!(e.dirty());
    ctrl(&mut e, 's');
    e.saved(Some("Shot 2.png".into()), now(&e));
    assert!(!e.dirty());
    assert_eq!(e.title(), "Shot 2.png");
    assert_eq!(e.notice.as_ref().map(|n| n.notice), Some(Notice::Saved));

    ctrl(&mut e, 'z');
    ctrl(&mut e, 's');
    e.failed("нет доступа к папке", now(&e));
    assert!(e.dirty());
    assert_eq!(
        e.notice.as_ref().map(|n| n.notice),
        Some(Notice::Failed("нет доступа к папке"))
    );

    ctrl(&mut e, 'c');
    assert_eq!(
        e.take_requests(),
        [Request::Save, Request::Save, Request::Copy]
    );
    let result = e.result();
    assert_eq!((result.width(), result.height()), (400, 300));
}

#[test]
fn every_edit_undoes_and_redoes() {
    let mut e = editor();
    let mut states = vec![e.objects().to_vec()];
    let edit = |e: &mut Editor, states: &mut Vec<Vec<Object>>, what: &dyn Fn(&mut Editor)| {
        what(e);
        assert_ne!(
            states.last().unwrap(),
            e.objects(),
            "an edit changes the objects"
        );
        states.push(e.objects().to_vec());
    };
    edit(&mut e, &mut states, &|e| {
        drag(e, at(20.0, 20.0), at(80.0, 60.0))
    });
    edit(&mut e, &mut states, &|e| {
        char(e, 'r');
        drag(e, at(100.0, 100.0), at(200.0, 160.0));
    });
    edit(&mut e, &mut states, &|e| {
        char(e, 'v');
        drag(e, at(100.0, 130.0), at(110.0, 140.0));
    });
    edit(&mut e, &mut states, &|e| {
        drag(e, at(210.0, 170.0), at(250.0, 200.0))
    });
    edit(&mut e, &mut states, &|e| {
        let b = button(e, Button::Color(2));
        click(e, b);
    });
    edit(&mut e, &mut states, &|e| {
        let b = button(e, Button::Width(Width::S));
        click(e, b);
    });
    // A series of nudges is one step.
    edit(&mut e, &mut states, &|e| {
        key(e, Key::Left, false, false);
        key(e, Key::Up, false, true);
        key(e, Key::Up, false, false);
    });
    edit(&mut e, &mut states, &|e| key(e, Key::Delete, false, false));

    for want in states.iter().rev().skip(1) {
        ctrl(&mut e, 'z');
        assert_eq!(e.objects(), want.as_slice());
    }
    ctrl(&mut e, 'z');
    assert!(e.objects().is_empty());
    assert!(!e.dirty());
    for want in &states[1..] {
        key(&mut e, Key::Char('z'), true, true);
        assert_eq!(e.objects(), want.as_slice());
    }

    // Saved after the rect was resized: undoing back to it is clean again.
    for _ in 0..4 {
        ctrl(&mut e, 'z');
    }
    assert_eq!(e.objects(), states[4].as_slice());
    ctrl(&mut e, 's');
    e.saved(None, now(&e));
    assert!(!e.dirty());
    ctrl(&mut e, 'y');
    assert!(e.dirty());
    ctrl(&mut e, 'z');
    assert!(!e.dirty());

    // A new edit drops what could be redone.
    char(&mut e, 'o');
    drag(&mut e, at(300.0, 200.0), at(350.0, 250.0));
    let drawn = e.objects().to_vec();
    assert_eq!(drawn.len(), 3);
    key(&mut e, Key::Char('z'), true, true);
    ctrl(&mut e, 'y');
    assert_eq!(e.objects(), drawn.as_slice());
    ctrl(&mut e, 'z');
    assert_eq!(e.objects(), states[4].as_slice());
}

#[test]
fn zoom_and_pan() {
    let mut e = editor();
    assert_eq!(e.zoom(), 1.0);
    let t = now(&e);
    let (x, y) = at(200.0, 150.0);
    e.handle(Input::Zoom { x, y, steps: 2 }, t);
    assert_eq!(e.zoom(), 2.0);
    // The image point under the cursor stays.
    assert!(e.view.to_image(x, y).dist(Pt::new(200.0, 150.0)) < 1.0);

    // Space and a drag pan.
    let before = e.view.shown();
    key(&mut e, Key::Space, false, false);
    e.handle(Input::Move { x, y, shift: false }, t);
    assert_eq!(e.cursor(), Cursor::Grab);
    e.handle(Input::Down { x, y, shift: false }, t);
    assert_eq!(e.cursor(), Cursor::Grabbing);
    e.handle(
        Input::Move {
            x: x + 30.0,
            y: y + 20.0,
            shift: false,
        },
        t,
    );
    e.handle(
        Input::Up {
            x: x + 30.0,
            y: y + 20.0,
            shift: false,
        },
        t,
    );
    e.handle(
        Input::KeyUp {
            key: Key::Space,
            shift: false,
        },
        t,
    );
    // 800 px wide fit the area: only the height scrolls.
    assert_eq!(e.view.shown(), before.offset(0, 20));
    assert!(e.objects().is_empty());

    e.handle(Input::Scroll { dx: 0.0, dy: -40.0 }, t);
    assert_eq!(e.view.shown().y, before.y + 20 - 40);

    ctrl(&mut e, '0');
    assert_eq!(e.zoom(), 1.0);
    e.handle(Input::Zoom { x, y, steps: -1 }, t);
    assert_eq!(e.zoom(), 0.67);
    // The label switches 100% and fit.
    let label = chrome::zoom_label_rect(e.window(), 1.0, 67);
    click(&mut e, ((label.x + 4) as f32, (label.y + 4) as f32));
    assert_eq!(e.zoom(), 1.0);
}

#[test]
fn a_tooltip_waits_and_a_press_hides_it() {
    let mut e = editor();
    let t0 = now(&e);
    let (x, y) = button(&e, Button::Save);
    e.handle(Input::Move { x, y, shift: false }, t0);
    assert_eq!(e.next_deadline(), Some(t0 + TOOLTIP_DELAY));
    e.take_damage();
    e.tick(t0 + TOOLTIP_DELAY);
    assert!(e.tooltip_sprite.is_some());
    assert!(!e.take_damage().is_empty());
    assert_eq!(e.next_deadline(), None);

    e.handle(Input::Down { x, y, shift: false }, t0 + TOOLTIP_DELAY);
    assert!(e.tooltip_sprite.is_none());
    // Colors have no tooltip.
    let (x, y) = button(&e, Button::Color(1));
    e.handle(Input::Up { x, y, shift: false }, t0 + TOOLTIP_DELAY);
    e.handle(Input::Move { x, y, shift: false }, t0 + TOOLTIP_DELAY);
    assert_eq!(e.next_deadline(), None);
}

#[test]
fn a_notice_fades_in_and_out() {
    let mut e = editor();
    let t0 = now(&e);
    e.copied(t0);
    assert_eq!(e.notice_alpha, 0);
    assert_eq!(e.next_deadline(), Some(t0 + FRAME));
    e.tick(t0 + FADE);
    assert_eq!(e.notice_alpha, 255);
    // Still: nothing to do until it fades out.
    assert_eq!(e.next_deadline(), Some(t0 + NOTICE - FADE));
    e.tick(t0 + NOTICE - FADE / 2);
    assert!((100..160).contains(&e.notice_alpha), "{}", e.notice_alpha);
    e.tick(t0 + NOTICE);
    assert!(e.notice.is_none() && e.notice_sprite.is_none());
    assert_eq!(e.next_deadline(), None);
}

#[test]
fn the_drag_out_button_drags_the_file() {
    let mut e = editor();
    let t = now(&e);
    let r = chrome::drag_out_rect(e.window(), 1.0);
    let (x, y) = ((r.x + r.width / 2) as f32, (r.y + r.height / 2) as f32);
    e.handle(Input::Move { x, y, shift: false }, t);
    assert_eq!(e.cursor(), Cursor::Grab);
    assert!(e.drag_sprite.is_some());

    // A press that moves less than a drag's reach is still a click.
    e.handle(Input::Down { x, y, shift: false }, t);
    assert_eq!(e.cursor(), Cursor::Grabbing);
    e.handle(
        Input::Move {
            x: x + 2.0,
            y,
            shift: false,
        },
        t,
    );
    assert!(e.take_requests().is_empty());
    // The drag asks for the file once and leaves the edits alone.
    e.handle(
        Input::Move {
            x: x + 12.0,
            y: y - 8.0,
            shift: false,
        },
        t,
    );
    e.handle(
        Input::Move {
            x: x + 40.0,
            y: y - 30.0,
            shift: false,
        },
        t,
    );
    assert_eq!(e.take_requests(), [Request::DragFile]);
    assert!(e.objects().is_empty());
    // The platform ran the drag; the button comes back as the pointer finds it.
    e.handle(Input::DragEnded, t);
    e.handle(Input::Move { x, y, shift: false }, t);
    assert_eq!(e.cursor(), Cursor::Grab);
    assert!(e.notice.is_none());

    // A click only explains the button.
    click(&mut e, (x, y));
    assert!(e.take_requests().is_empty());
    assert_eq!(e.notice.as_ref().map(|n| n.notice), Some(Notice::DragHint));
    // A press on the button that ends elsewhere does nothing.
    let mut e = editor();
    e.handle(Input::Down { x, y, shift: false }, t);
    e.handle(Input::Blur, t);
    assert!(e.take_requests().is_empty() && e.notice.is_none());
}

/// Draws the whole window.
fn full(e: &Editor) -> Pixmap {
    let (w, h) = e.window();
    let mut out = Pixmap::new(w, h).unwrap();
    e.render(&mut out, Rect::new(0, 0, w as i32, h as i32));
    out
}

/// Redraws only the damage into `frame`, as the platform does: each part with a margin of
/// [`EDGE`] that is not shown.
fn repaint(e: &mut Editor, frame: &mut Pixmap) -> i64 {
    let mut area = 0;
    for r in e.take_damage() {
        area += r.area();
        let canvas = r.inflate(EDGE);
        let mut part = Pixmap::new(canvas.width as u32, canvas.height as u32).unwrap();
        e.render(&mut part, canvas);
        let (pw, w) = (canvas.width as usize * 4, frame.width() as usize * 4);
        for row in 0..r.height as usize {
            let from = (row + EDGE as usize) * pw + EDGE as usize * 4;
            let src = &part.data()[from..from + r.width as usize * 4];
            let start = (r.y as usize + row) * w + r.x as usize * 4;
            frame.data_mut()[start..start + src.len()].copy_from_slice(src);
        }
    }
    area
}

/// The colors of two frames, alpha bytes aside.
fn same(a: &Pixmap, b: &Pixmap) -> bool {
    a.data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data().as_chunks::<4>().0)
        .all(|(p, q)| p[..3] == q[..3])
}

#[test]
fn redrawing_the_damage_matches_a_full_redraw() {
    let mut e = editor();
    let mut frame = Pixmap::new(960, 600).unwrap();
    assert_eq!(repaint(&mut e, &mut frame), 960 * 600);

    let t = now(&e);
    type Step = Box<dyn Fn(&mut Editor)>;
    let steps: Vec<Step> = vec![
        Box::new(|e| drag(e, at(20.0, 30.0), at(120.0, 80.0))),
        Box::new(|e| char(e, 'r')),
        Box::new(|e| drag(e, at(200.0, 100.0), at(300.0, 200.0))),
        Box::new(|e| char(e, 'v')),
        Box::new(|e| drag(e, at(200.0, 150.0), at(220.0, 170.0))),
        Box::new(|e| key(e, Key::Left, false, true)),
        Box::new(|e| {
            let b = button(e, Button::Color(4));
            click(e, b)
        }),
        Box::new(|e| ctrl(e, 'z')),
        Box::new(|e| click(e, at(5.0, 5.0))),
        Box::new(|e| {
            e.handle(
                Input::Zoom {
                    x: 500.0,
                    y: 300.0,
                    steps: 1,
                },
                e.now,
            )
        }),
        Box::new(|e| {
            e.handle(
                Input::Scroll {
                    dx: -25.0,
                    dy: 10.0,
                },
                e.now,
            )
        }),
        Box::new(|e| e.copied(e.now)),
        Box::new(|e| e.tick(e.now + FADE / 2)),
        Box::new(|e| key(e, Key::Escape, false, false)),
        Box::new(|e| key(e, Key::Escape, false, false)),
    ];
    for (i, step) in steps.iter().enumerate() {
        step(&mut e);
        repaint(&mut e, &mut frame);
        assert!(same(&frame, &full(&e)), "step {i}");
    }
    // Moving over the image with nothing under the cursor redraws nothing.
    e.handle(
        Input::Move {
            x: 600.0,
            y: 400.0,
            shift: false,
        },
        t,
    );
    e.handle(
        Input::Move {
            x: 610.0,
            y: 410.0,
            shift: false,
        },
        t,
    );
    e.take_damage();
    e.handle(
        Input::Move {
            x: 620.0,
            y: 420.0,
            shift: false,
        },
        t,
    );
    assert!(e.take_damage().is_empty());
}

#[test]
fn drawing_redraws_a_small_part() {
    let mut e = editor();
    let mut frame = Pixmap::new(960, 600).unwrap();
    repaint(&mut e, &mut frame);
    let t = now(&e);
    let (x, y) = at(100.0, 100.0);
    e.handle(Input::Down { x, y, shift: false }, t);
    e.handle(
        Input::Move {
            x: x + 40.0,
            y: y + 30.0,
            shift: false,
        },
        t,
    );
    repaint(&mut e, &mut frame);
    e.handle(
        Input::Move {
            x: x + 50.0,
            y: y + 35.0,
            shift: false,
        },
        t,
    );
    let area = repaint(&mut e, &mut frame);
    assert!(area > 0 && area < 120 * 100, "{area}");
    assert!(same(&frame, &full(&e)));
}

#[test]
fn hover_outlines_and_the_theme_switches() {
    let mut e = editor();
    drag(&mut e, at(20.0, 30.0), at(120.0, 80.0));
    char(&mut e, 'v');
    click(&mut e, at(5.0, 290.0));
    assert_eq!(e.selected(), None);
    e.take_damage();
    let t = now(&e);
    let (x, y) = at(70.0, 55.0);
    e.handle(Input::Move { x, y, shift: false }, t);
    assert_eq!(e.hovered, Some(0));
    assert!(!e.take_damage().is_empty());
    e.handle(Input::Leave, t);
    assert_eq!(e.hovered, None);

    e.handle(Input::Theme { dark: true }, t);
    let damage = e.take_damage();
    assert_eq!(damage, [Rect::new(0, 0, 960, 600)]);
    e.handle(
        Input::Resize {
            width: 1200,
            height: 800,
            scale: 1.25,
        },
        t,
    );
    assert_eq!(e.window(), (1200, 800));
    assert_eq!(e.take_damage(), [Rect::new(0, 0, 1200, 800)]);
}
