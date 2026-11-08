//! The event-rate breaker over a fake clock: it trips at exactly the window, a spike that
//! breaks off doesn't trip it, and it resumes after the pause.

use std::time::Duration;

use super::super::breaker::{Action, Breaker, PAUSE, PAUSE_RATE, WINDOW_SECONDS};
use super::super::protocol::{Line, Paused, paused_line, resumed_line};

/// Steps `breaker` once a second from `from` for `seconds`, `rate` events each second;
/// gives each second's action with the time, and the count reached.
fn run(
    breaker: &mut Breaker,
    from: u64,
    seconds: u64,
    rate: u64,
    count: &mut u64,
) -> Vec<(u64, Action)> {
    let mut actions = Vec::new();
    for s in from + 1..=from + seconds {
        *count += rate;
        let step = breaker.step(Duration::from_secs(s), *count);
        assert_eq!(step.second, Some(rate));
        if let Some(action) = step.action {
            actions.push((s, action));
        }
    }
    actions
}

#[test]
fn it_trips_at_exactly_the_window_and_resumes_after_the_pause() {
    let mut breaker = Breaker::new(Duration::ZERO, 0);
    let mut count = 0;
    let window = u64::from(WINDOW_SECONDS);
    let actions = run(&mut breaker, 0, window - 1, PAUSE_RATE, &mut count);
    assert_eq!(actions, []);
    let actions = run(&mut breaker, window - 1, 1, PAUSE_RATE, &mut count);
    assert_eq!(actions, [(window, Action::Pause)]);
    assert_eq!(breaker.trips(), 1);
    // Paused: whatever the rate, nothing until the pause has passed.
    let pause = PAUSE.as_secs();
    let actions = run(&mut breaker, window, pause - 1, PAUSE_RATE * 2, &mut count);
    assert_eq!(actions, []);
    let actions = run(&mut breaker, window + pause - 1, 1, 0, &mut count);
    assert_eq!(actions, [(window + pause, Action::Resume)]);
    // After it resumes, the window counts from zero again.
    let from = window + pause;
    let actions = run(&mut breaker, from, window, PAUSE_RATE, &mut count);
    assert_eq!(actions, [(from + window, Action::Pause)]);
    assert_eq!(breaker.trips(), 2);
}

#[test]
fn a_spike_that_breaks_off_doesnt_trip_it() {
    let mut breaker = Breaker::new(Duration::ZERO, 0);
    let mut count = 0;
    let mut at = 0;
    for _ in 0..10 {
        let window = u64::from(WINDOW_SECONDS);
        assert_eq!(
            run(&mut breaker, at, window - 1, PAUSE_RATE * 3, &mut count),
            []
        );
        assert_eq!(
            run(&mut breaker, at + window - 1, 1, PAUSE_RATE - 1, &mut count),
            []
        );
        at += window;
    }
    assert_eq!(breaker.trips(), 0);
}

#[test]
fn the_rate_is_per_second_whatever_the_tick() {
    let mut breaker = Breaker::new(Duration::ZERO, 0);
    // A quarter-second tick: no bucket ends until a second has passed.
    assert_eq!(breaker.step(Duration::from_millis(750), 100).second, None);
    // A bucket of 1.25 s with 10,000 events is 8,000 a second.
    assert_eq!(
        breaker.step(Duration::from_millis(1250), 10_000).second,
        Some(8000)
    );
}

#[test]
fn the_pause_lines_are_as_the_protocol_says() {
    assert_eq!(
        paused_line(60),
        "{\"v\":1,\"paused\":{\"secs\":60,\"reason\":\"rate\"}}\n"
    );
    assert_eq!(resumed_line(), "{\"v\":1,\"resumed\":true}\n");
    assert_eq!(
        Line::parse(paused_line(60).trim_end()),
        Some(Line::Paused {
            v: 1,
            paused: Paused {
                secs: 60,
                reason: "rate".to_owned()
            }
        })
    );
    assert_eq!(
        Line::parse(resumed_line().trim_end()),
        Some(Line::Resumed {
            v: 1,
            resumed: true
        })
    );
}
