//! The menu's page commands: their ids, the script that sends them, and the page's side, which
//! must hear the same event and names (`ui/app/src/room/room-commands.ts`). The bar itself is not
//! built here: muda builds a menu on the main thread only, which a test is not.

use tauri::test::{MockRuntime, mock_builder};
use tauri::{WebviewUrl, WebviewWindowBuilder};

use super::*;

/// The page's file that hears the menu.
fn page() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/app/src/room/room-commands.ts");
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn each_command_has_its_own_id_and_is_found_by_it() {
    for command in PageCommand::ALL {
        assert_eq!(PageCommand::of_id(command.id()), Some(command));
    }
    assert_eq!(PageCommand::of_id("settings"), None);
    assert_eq!(PageCommand::of_id(""), None);
    assert_eq!(PageCommand::of_id("New_matter"), None);
    assert_eq!(
        PageCommand::of_id("toggle_sidebar"),
        Some(PageCommand::ToggleSidebar)
    );
    assert_eq!(
        PageCommand::of_id("toggle_papers"),
        Some(PageCommand::TogglePapers)
    );
    assert_eq!(PageCommand::of_id("toggle"), None);
}

#[test]
fn only_the_panel_toggles_leave_the_window_where_it_is() {
    for command in PageCommand::ALL {
        let toggle = matches!(
            command,
            PageCommand::ToggleSidebar | PageCommand::TogglePapers
        );
        assert_eq!(command.raises(), !toggle, "{command:?}");
    }
}

#[test]
fn the_toggles_say_what_a_press_does_in_both_languages() {
    use clauseo_protocol::Language::{En, Hi};
    let layout = |sidebar, papers| LayoutShown { sidebar, papers };
    let cases = [
        (
            PageCommand::ToggleSidebar,
            true,
            "Hide the sidebar",
            "साइडबार छिपाएँ",
        ),
        (
            PageCommand::ToggleSidebar,
            false,
            "Show the sidebar",
            "साइडबार दिखाएँ",
        ),
        (
            PageCommand::TogglePapers,
            true,
            "Hide the papers",
            "दस्तावेज़ छिपाएँ",
        ),
        (
            PageCommand::TogglePapers,
            false,
            "Show the papers",
            "दस्तावेज़ दिखाएँ",
        ),
    ];
    for (command, shown, en, hi) in cases {
        // each toggle reads its own flag, whatever the other's
        for other in [true, false] {
            let flags = if command == PageCommand::ToggleSidebar {
                layout(shown, other)
            } else {
                layout(other, shown)
            };
            assert_eq!(toggle_text(En, command, flags), Some(en), "{flags:?}");
            assert_eq!(toggle_text(Hi, command, flags), Some(hi), "{flags:?}");
        }
    }
    for command in [
        PageCommand::NewConversation,
        PageCommand::NewMatter,
        PageCommand::Find,
    ] {
        assert_eq!(toggle_text(En, command, LayoutShown::BOTH), None);
    }
    // the bar opens with both shown: the Hide words
    assert_eq!(LayoutShown::BOTH, layout(true, true));
}

#[test]
fn the_script_sends_the_pages_event_with_the_command_and_nothing_else() {
    assert_eq!(
        script(PageCommand::NewMatter),
        "window.dispatchEvent(new CustomEvent(\"clauseo-menu\", { detail: \"new_matter\" }));"
    );
    for command in PageCommand::ALL {
        let script = script(command);
        assert_eq!(script.matches("dispatchEvent").count(), 1, "{script}");
        assert!(script.ends_with(&format!("{{ detail: \"{}\" }}));", command.id())));
    }
    assert_eq!(
        script(PageCommand::ToggleSidebar),
        "window.dispatchEvent(new CustomEvent(\"clauseo-menu\", { detail: \"toggle_sidebar\" }));"
    );
    assert_eq!(
        script(PageCommand::TogglePapers),
        "window.dispatchEvent(new CustomEvent(\"clauseo-menu\", { detail: \"toggle_papers\" }));"
    );
}

#[test]
fn the_page_hears_the_event_and_every_command() {
    let page = page();
    assert!(
        page.contains(&format!("export const MENU_EVENT = \"{EVENT}\";")),
        "room-commands.ts names another event"
    );
    for command in PageCommand::ALL {
        let name = format!("\"{}\"", command.id());
        // in the page's type and in its handler
        assert!(
            page.matches(&name).count() >= 2,
            "the page doesn't run {name}"
        );
    }
}

#[test]
fn a_command_with_no_main_window_does_nothing_and_with_one_reaches_it() {
    let app = mock_builder()
        .build(crate::context::<MockRuntime>())
        .unwrap();
    send(&app, PageCommand::Find).unwrap();
    WebviewWindowBuilder::new(&app, crate::MAIN_WINDOW, WebviewUrl::default())
        .build()
        .unwrap();
    for command in PageCommand::ALL {
        send(&app, command).unwrap();
    }
}
