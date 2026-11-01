use super::{Capture, Input, Look, Pointer, Step, lparam_point, menu_word, step};

#[test]
fn lparam_points_are_signed_words() {
    assert_eq!(lparam_point(0), (0, 0));
    assert_eq!(lparam_point((0x20 << 16) | 0x2E), (46, 32));
    // (-1, -2): a captured pointer above and left of the window.
    assert_eq!(lparam_point(0xFFFE_FFFF), (-1, -2));
    assert_eq!(lparam_point(0x7FFF_8000), (-0x8000, 0x7FFF));
}

/// Runs `inputs` from `Away` and returns every step.
fn run(inputs: &[Input]) -> Vec<Step> {
    let mut now = Pointer::Away;
    inputs
        .iter()
        .map(|&input| {
            let next = step(now, input);
            now = next.next;
            next
        })
        .collect()
}

/// The toggles in `steps`.
fn toggles(steps: &[Step]) -> usize {
    steps.iter().filter(|s| s.toggle).count()
}

#[test]
fn hovering_draws_the_hover_once_and_asks_for_the_leave() {
    let steps = run(&[Input::Move, Input::Move, Input::Move]);
    assert_eq!(steps[0].look, Some(Look::Hover));
    assert!(steps[0].track_leave);
    assert_eq!(
        steps[1],
        Step {
            next: Pointer::Hover,
            ..Step::default()
        }
    );
    assert_eq!(
        steps[2],
        Step {
            next: Pointer::Hover,
            ..Step::default()
        }
    );
}

#[test]
fn leaving_clears_the_hover() {
    let steps = run(&[Input::Move, Input::Leave]);
    assert_eq!(steps[1].look, Some(Look::Rest));
    assert_eq!(steps[1].next, Pointer::Away);
}

#[test]
fn a_press_and_release_over_the_button_toggles_once_and_releases() {
    let steps = run(&[
        Input::Move,
        Input::Down,
        Input::Up { over: true },
        Input::CaptureLost,
    ]);
    assert_eq!(steps[1].look, Some(Look::Press));
    assert_eq!(steps[1].capture, Capture::Take);
    assert!(steps[2].toggle);
    assert_eq!(steps[2].capture, Capture::Release);
    assert_eq!(steps[2].look, Some(Look::Rest));
    // The release's own WM_CAPTURECHANGED changes nothing.
    assert_eq!(steps[3], Step::default());
    assert_eq!(toggles(&steps), 1);
}

#[test]
fn a_press_dragged_off_and_released_there_does_not_toggle() {
    let steps = run(&[
        Input::Down,
        Input::Drag { over: false },
        Input::Up { over: false },
    ]);
    assert_eq!(steps[1].look, Some(Look::Rest));
    assert_eq!(steps[1].next, Pointer::PressOff);
    assert_eq!(steps[2].capture, Capture::Release);
    assert_eq!(toggles(&steps), 0);
}

#[test]
fn a_press_dragged_off_and_back_presses_again_and_toggles() {
    let steps = run(&[
        Input::Down,
        Input::Drag { over: false },
        Input::Drag { over: true },
        Input::Up { over: true },
    ]);
    assert_eq!(steps[2].look, Some(Look::Press));
    assert_eq!(toggles(&steps), 1);
}

#[test]
fn a_lost_capture_cancels_the_press() {
    let steps = run(&[Input::Down, Input::CaptureLost, Input::Up { over: true }]);
    assert_eq!(steps[1].look, Some(Look::Rest));
    assert_eq!(steps[1].next, Pointer::Away);
    assert_eq!(toggles(&steps), 0);
}

#[test]
fn a_leave_while_pressed_keeps_the_press() {
    let steps = run(&[
        Input::Move,
        Input::Down,
        Input::Leave,
        Input::Up { over: true },
    ]);
    assert_eq!(
        steps[2],
        Step {
            next: Pointer::Press,
            ..Step::default()
        }
    );
    assert_eq!(toggles(&steps), 1);
}

#[test]
fn client_messages_without_a_press_do_nothing() {
    for input in [
        Input::Drag { over: true },
        Input::Up { over: true },
        Input::CaptureLost,
        Input::Leave,
    ] {
        assert_eq!(step(Pointer::Away, input), Step::default(), "{input:?}");
        assert!(
            !step(Pointer::Hover, input).toggle,
            "{input:?} toggled from Hover"
        );
    }
}

#[test]
fn the_scripts_are_fixed_and_name_only_the_maximise_button() {
    for look in [Look::Rest, Look::Hover, Look::Press] {
        let script = look.script();
        assert!(script.starts_with("document.querySelector('[data-caption=\"maximise\"]')?."));
        assert!(script.contains("data-pointer"));
    }
    assert!(
        Look::Hover
            .script()
            .ends_with("setAttribute('data-pointer','hover')")
    );
    assert!(
        Look::Press
            .script()
            .ends_with("setAttribute('data-pointer','press')")
    );
    assert!(
        Look::Rest
            .script()
            .ends_with("removeAttribute('data-pointer')")
    );
}

#[test]
fn menu_words_lose_the_accelerator_and_the_shortcut() {
    assert_eq!(menu_word("Ma&ximize"), "Maximize");
    assert_eq!(menu_word("&Restore"), "Restore");
    assert_eq!(menu_word("&Close\tAlt+F4"), "Close");
    assert_eq!(menu_word("最大化(&X)"), "最大化");
    assert_eq!(menu_word("Save && &Quit"), "Save & Quit");
    assert_eq!(menu_word("अधिकतम करें(&X)"), "अधिकतम करें");
    assert_eq!(menu_word(" Agrandir "), "Agrandir");
}

#[test]
fn an_empty_or_bare_item_gives_no_name() {
    assert_eq!(menu_word(""), "");
    assert_eq!(menu_word("&"), "");
    assert_eq!(menu_word("\tAlt+F10"), "");
    // A lone "(&" at the end is kept apart from its "&".
    assert_eq!(menu_word("Zoom (&"), "Zoom (");
}
