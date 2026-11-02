//! The display settings over the wire: `GetSettings`, `SetUiSettings` and `settings.locked`,
//! and the app subscription that carries `SettingsChanged` to every window and to no stream.
//! The mirror test holds the wire types to `clauseo-settings`, key for key and value for value.

use std::collections::BTreeSet;

use clauseo_host::{AppSubscription, EncodedEvent};
use clauseo_protocol::{
    CommandOutcome, ErrorCode, HostCommand, HostEvent, HostQuery, Language, Lite, Motion,
    Principal, QueryResult, SettingsView, StreamId, Theme, UiPatch, UiSettings,
};
use futures::{FutureExt, StreamExt};
use serde_json::Value;

use crate::support::{Dir, FAKE, Running, Scenario, command_id, matter_with_thread, wait};

/// The settings `GetSettings` gives.
fn settings(host: &Running) -> SettingsView {
    match host.query(HostQuery::GetSettings).unwrap() {
        QueryResult::Settings(view) => view,
        other => panic!("GetSettings answered {other:?}"),
    }
}

/// `SetUiSettings` with `patch`.
fn set(patch: UiPatch) -> HostCommand {
    HostCommand::SetUiSettings { patch }
}

/// An app subscription as the local desktop.
fn app(host: &Running) -> AppSubscription {
    wait(
        "subscribe_app",
        host.handle.subscribe_app(Principal::LocalDesktop),
    )
    .unwrap_or_else(|e| panic!("subscribe_app: {e}"))
}

/// What is already in `sub`'s channel, decoded, without waiting. Call after `host.settle()`.
fn ready(sub: &mut AppSubscription) -> Vec<HostEvent> {
    let mut events = Vec::new();
    while let Some(Some(item)) = sub.next().now_or_never() {
        events.push(decode(&item));
    }
    events
}

fn decode(item: &EncodedEvent) -> HostEvent {
    item.decode()
        .unwrap_or_else(|e| panic!("{e}: {}", item.json()))
}

/// The defaults, as `clauseo-settings` has them.
fn defaults() -> UiSettings {
    UiSettings {
        theme: Theme::System,
        font_size: 16,
        language: Language::En,
        lite: Lite::Automatic,
        motion: Motion::System,
    }
}

#[test]
fn get_settings_gives_the_defaults_and_no_locked_key() {
    let dir = Dir::new("get_settings_defaults", &[]);
    let host = dir.start(0);
    assert_eq!(
        settings(&host),
        SettingsView {
            ui: defaults(),
            locked: Vec::new(),
        }
    );
    host.shutdown();
}

#[test]
fn get_settings_gives_policy_values_and_the_locked_keys_sorted() {
    let dir = Dir::new("get_settings_policy", &[]);
    let host = dir.start_with_policy(
        0,
        r#"{"ui": {"theme": "dark", "font_size": 18}, "matters": {"default_location": null}}"#,
    );
    let view = settings(&host);
    assert_eq!(view.ui.theme, Theme::Dark);
    assert_eq!(view.ui.font_size, 18);
    assert_eq!(
        view.locked,
        ["matters.default_location", "ui.font_size", "ui.theme"]
    );
    host.shutdown();
}

#[test]
fn a_patch_reaches_every_app_subscriber_and_no_stream() {
    let dir = Dir::new(
        "settings_patch_reaches_app",
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
    );
    let host = dir.start(0);
    let (matter, thread, _) = matter_with_thread(&dir, &host, "m", &[("a.txt", "x\n")], FAKE);
    // Every kind of stream subscription, on the thread and on the matter. Each panics on a
    // `SettingsChanged`, since it takes only its own kind of event.
    let mut thread_view = host.view(StreamId::Thread(thread), None);
    let mut matter_view = host.view(StreamId::Matter(matter), None);
    let mut thread_events = host.events(StreamId::Thread(thread));
    let mut matter_events = host.events(StreamId::Matter(matter));
    thread_view.until_items(1);
    matter_view.until_items(1);
    thread_events.until_caught_up();
    matter_events.until_caught_up();
    let mut first = app(&host);
    let mut second = app(&host);

    let id = command_id();
    let patch = UiPatch {
        theme: Some(Theme::Dark),
        font_size: Some(20),
        language: Some(Language::Hi),
        lite: Some(Lite::Off),
        motion: Some(Motion::Reduce),
    };
    assert_eq!(
        host.dispatch_as(id, set(patch.clone())).unwrap(),
        CommandOutcome::Done
    );
    let expected = SettingsView {
        ui: UiSettings {
            theme: Theme::Dark,
            font_size: 20,
            language: Language::Hi,
            lite: Lite::Off,
            motion: Motion::Reduce,
        },
        locked: Vec::new(),
    };
    assert_eq!(settings(&host), expected);
    host.settle();
    for sub in [&mut first, &mut second] {
        assert_eq!(ready(sub), [HostEvent::SettingsChanged(expected.clone())]);
    }
    let (thread_items, matter_items) = (thread_view.items.len(), matter_view.items.len());
    let (thread_seen, matter_seen) = (thread_events.seen.len(), matter_events.seen.len());
    thread_view.drain_ready();
    matter_view.drain_ready();
    thread_events.drain_ready();
    matter_events.drain_ready();
    assert_eq!(thread_view.items.len(), thread_items);
    assert_eq!(matter_view.items.len(), matter_items);
    assert_eq!(thread_events.seen.len(), thread_seen);
    assert_eq!(matter_events.seen.len(), matter_seen);

    // The write reached settings.json beside the agents it already held, so the next start reads
    // it back.
    let saved: Value = serde_json::from_str(&dir.settings_json()).unwrap();
    assert_eq!(saved["ui"]["theme"], "dark");
    assert_eq!(saved["ui"]["font_size"], 20);
    assert!(saved["agents"][FAKE].is_object(), "{saved}");

    // A repeat of the same command answers from its receipt: nothing is sent again.
    assert_eq!(
        host.dispatch_as(id, set(patch)).unwrap(),
        CommandOutcome::Done
    );
    // An empty patch writes nothing and tells nobody.
    assert_eq!(host.ok(set(UiPatch::default())), CommandOutcome::Done);
    host.settle();
    assert_eq!(ready(&mut first), []);
    host.shutdown();

    let host = dir.start(1);
    assert_eq!(settings(&host), expected);
    host.shutdown();
}

#[test]
fn a_patch_naming_a_locked_key_is_refused_whole() {
    let dir = Dir::new("settings_locked", &[]);
    let host = dir.start_with_policy(0, r#"{"ui": {"theme": "light"}}"#);
    let mut sub = app(&host);
    let before = dir.settings_json();

    // The font size alone would be allowed; with the locked theme the patch fails whole, even
    // when it asks for the value the policy already set.
    for theme in [Theme::Dark, Theme::Light] {
        let error = host.refused(
            set(UiPatch {
                theme: Some(theme),
                font_size: Some(18),
                ..UiPatch::default()
            }),
            ErrorCode::SettingsLocked,
        );
        assert!(!error.retryable());
        assert!(error.message().contains("ui.theme"), "{error}");
    }
    assert_eq!(dir.settings_json(), before);
    let view = settings(&host);
    assert_eq!(view.ui.theme, Theme::Light);
    assert_eq!(view.ui.font_size, 16);
    assert_eq!(view.locked, ["ui.theme"]);
    host.settle();
    assert_eq!(ready(&mut sub), []);

    // A patch of unlocked keys still works, and its event carries the locked list.
    host.ok(set(UiPatch {
        font_size: Some(18),
        ..UiPatch::default()
    }));
    host.settle();
    let changed = ready(&mut sub);
    let [HostEvent::SettingsChanged(view)] = changed.as_slice() else {
        panic!("expected one SettingsChanged: {changed:?}");
    };
    assert_eq!(view.ui.font_size, 18);
    assert_eq!(view.ui.theme, Theme::Light);
    assert_eq!(view.locked, ["ui.theme"]);
    host.shutdown();
}

#[test]
fn a_font_size_out_of_range_is_refused_as_invalid() {
    let dir = Dir::new("settings_font_range", &[]);
    let host = dir.start(0);
    let mut sub = app(&host);
    let before = dir.settings_json();
    // 12 and 13 were allowed before the range moved to 14 to 20.
    for size in [0, 12, 13, 21, 255] {
        let error = host.refused(
            set(UiPatch {
                font_size: Some(size),
                theme: Some(Theme::Dark),
                ..UiPatch::default()
            }),
            ErrorCode::CommandInvalid,
        );
        assert_eq!(
            error.message(),
            format!("a font size is from 14 to 20, not {size}")
        );
    }
    assert_eq!(dir.settings_json(), before);
    assert_eq!(settings(&host).ui, defaults());
    host.settle();
    assert_eq!(ready(&mut sub), []);
    for size in [14, 20] {
        host.ok(set(UiPatch {
            font_size: Some(size),
            ..UiPatch::default()
        }));
        assert_eq!(settings(&host).ui.font_size, size);
    }
    host.shutdown();
}

#[test]
fn shutdown_ends_the_app_stream() {
    let dir = Dir::new("settings_app_shutdown", &[]);
    let host = dir.start(0);
    let mut sub = app(&host);
    host.shutdown();
    assert!(wait("the end of the app stream", sub.next()).is_none());
}

#[test]
fn an_unknown_principal_may_not_subscribe_to_the_app() {
    let dir = Dir::new("settings_app_principal", &[]);
    let host = dir.start(0);
    let phone: Principal = serde_json::from_str("\"some_phone\"").unwrap();
    let error = wait("subscribe_app", host.handle.subscribe_app(phone)).unwrap_err();
    assert_eq!(error.code(), ErrorCode::TransportUnauthorized);
    host.shutdown();
}

/// The app subscription capacity the slow-subscriber test sets, so a few changes fill it.
const SMALL_CAPACITY: usize = 4;

/// The font size of each `SettingsChanged` in `events`.
fn font_sizes(events: &[HostEvent]) -> Vec<u8> {
    events
        .iter()
        .map(|event| match event {
            HostEvent::SettingsChanged(view) => view.ui.font_size,
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

#[test]
fn a_slow_app_subscriber_is_caught_up_with_the_current_settings() {
    let dir = Dir::new("settings_app_slow", &[]);
    let host = dir.start_with(0, |config| {
        config.with_app_subscription_capacity(SMALL_CAPACITY)
    });
    let mut slow = app(&host);
    let mut other = app(&host);
    // 14 to 17 fill each channel, 18 finds it full and marks it lagged, 19 is not sent to it.
    for size in 14..=19 {
        host.ok(set(UiPatch {
            font_size: Some(size),
            ..UiPatch::default()
        }));
    }
    host.settle();
    // The channel's four, then, once drained, the settings as they are now: 19, never 18.
    let received = wait("the slow subscriber's events", async {
        let mut received = Vec::new();
        while received.len() < SMALL_CAPACITY + 1 {
            let item = slow.next().await.expect("the app stream ended");
            received.push(decode(&item));
        }
        received
    });
    assert_eq!(font_sizes(&received), [14, 15, 16, 17, 19]);
    host.settle();
    assert_eq!(ready(&mut slow), []);
    // The other subscriber lagged on its own; draining it catches it up the same way.
    let mut caught = ready(&mut other);
    assert_eq!(font_sizes(&caught), [14, 15, 16, 17]);
    let next = wait("the other subscriber's catch-up", other.next()).expect("ended");
    caught.push(decode(&next));
    assert_eq!(font_sizes(&caught), [14, 15, 16, 17, 19]);
    host.shutdown();
}

/// The `ui` fields `clauseo-settings` has and the wire leaves out on purpose: the app no longer
/// uses density.
const NOT_ON_THE_WIRE: [&str; 1] = ["density"];

/// The keys of a JSON object, less [`NOT_ON_THE_WIRE`].
fn keys(value: &Value) -> BTreeSet<String> {
    value
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| !NOT_ON_THE_WIRE.contains(&key.as_str()))
        .cloned()
        .collect()
}

/// The values `clauseo-settings`' JSON Schema allows for its enum `name`.
fn schema_values(name: &str) -> Vec<String> {
    let schema = clauseo_settings::json_schema();
    schema["$defs"][name]["oneOf"]
        .as_array()
        .unwrap_or_else(|| panic!("no enum {name} in the settings schema"))
        .iter()
        .map(|choice| choice["const"].as_str().unwrap().to_owned())
        .collect()
}

/// The JSON strings of `values`, each a JSON value.
fn strings(values: impl IntoIterator<Item = serde_json::Result<Value>>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.unwrap().as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn the_wire_mirrors_the_settings_crate_key_for_key_and_value_for_value() {
    // Keys: the settings crate's `ui` section and its patch, against the wire's.
    let settings_ui = serde_json::to_value(clauseo_settings::UiSettings::default()).unwrap();
    let wire_ui = serde_json::to_value(defaults()).unwrap();
    assert_eq!(keys(&settings_ui), keys(&wire_ui));
    let settings_patch = clauseo_settings::UiPatch {
        theme: Some(clauseo_settings::Theme::Dark),
        font_size: Some(clauseo_settings::FontSize::try_from(18).unwrap()),
        density: Some(clauseo_settings::Density::Compact),
        language: Some(clauseo_settings::Language::Hi),
        lite: Some(clauseo_settings::Lite::On),
        motion: Some(clauseo_settings::Motion::Full),
    };
    let wire_patch = UiPatch {
        theme: Some(Theme::Dark),
        font_size: Some(18),
        language: Some(Language::Hi),
        lite: Some(Lite::On),
        motion: Some(Motion::Full),
    };
    let settings_patch = serde_json::to_value(settings_patch).unwrap();
    let wire_patch = serde_json::to_value(wire_patch).unwrap();
    assert_eq!(keys(&settings_patch), keys(&wire_patch));
    // The same values write the same JSON, field by field.
    for key in keys(&wire_patch) {
        assert_eq!(settings_patch[&key], wire_patch[&key], "{key}");
    }

    // Values: every value of each enum, from the settings schema and from the wire's `ALL`.
    assert_eq!(
        schema_values("Theme"),
        strings(Theme::ALL.map(serde_json::to_value))
    );
    assert_eq!(
        schema_values("Language"),
        strings(Language::ALL.map(serde_json::to_value))
    );
    assert_eq!(
        schema_values("Lite"),
        strings(Lite::ALL.map(serde_json::to_value))
    );
    assert_eq!(
        schema_values("Motion"),
        strings(Motion::ALL.map(serde_json::to_value))
    );

    // Defaults: the settings crate's, and what a fresh host answers.
    assert_eq!(
        settings_ui["font_size"],
        clauseo_settings::FontSize::DEFAULT
    );
    for key in keys(&wire_ui) {
        assert_eq!(settings_ui[&key], wire_ui[&key], "{key}");
    }
    let dir = Dir::new("settings_mirror", &[]);
    let host = dir.start(0);
    assert_eq!(serde_json::to_value(settings(&host).ui).unwrap(), wire_ui);
    host.shutdown();
}
