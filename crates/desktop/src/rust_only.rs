//! `RustOnly`: a plugin minus the script it would inject into the page.
//!
//! Some plugins add a script to every page so their JavaScript APIs work. notification replaces
//! `window.Notification` and asks Rust for permission on load; dialog replaces `alert` and
//! `confirm`; opener turns every `target="_blank"` link click into a native open; os puts OS
//! details in a global. This app calls plugins from Rust only, so those scripts are dead weight
//! on every page load and turn plain web features into native actions. Wrapping a plugin in
//! `RustOnly` keeps everything it does in Rust and drops the script.
//!
//! Only the script goes: a plugin's commands stay registered (`extend_api` is passed through), so
//! the page can still invoke whatever `capabilities/main.json` grants, which today is nothing.
//!
//! `plugin_rust_only` is the one way to register a plugin. clippy's `disallowed-methods`
//! (`clippy.toml`) forbids `Builder::plugin`, `Builder::plugin_boxed`, `AppHandle::plugin` and
//! `AppHandle::plugin_boxed` everywhere else. The same list forbids the runtime permission grants
//! (`Manager::add_capability`, `Context::runtime_authority_mut`,
//! `RuntimeAuthority::add_capability`) and `Context::config_mut`.

use tauri::ipc::Invoke;
use tauri::plugin::Plugin;
use tauri::webview::PageLoadPayload;
use tauri::{AppHandle, RunEvent, Runtime, Url, Webview, Window};
use tauri_runtime::webview::InitializationScript;

/// Registers a plugin inside `RustOnly`.
pub(crate) trait PluginRustOnly<R: Runtime> {
    /// `Builder::plugin`, with the plugin's page script dropped.
    fn plugin_rust_only(self, plugin: impl Plugin<R> + 'static) -> Self;
}

impl<R: Runtime> PluginRustOnly<R> for tauri::Builder<R> {
    #[expect(
        clippy::disallowed_methods,
        reason = "the one place a plugin is registered, always inside RustOnly"
    )]
    fn plugin_rust_only(self, plugin: impl Plugin<R> + 'static) -> Self {
        self.plugin(RustOnly::new(plugin))
    }
}

/// A plugin with every hook passed through except its page script. It holds the plugin as a trait
/// object, so the wrapper is compiled once rather than once per plugin type, which keeps the
/// edit-to-binary loop as fast as without it.
struct RustOnly<R: Runtime>(Box<dyn Plugin<R>>);

impl<R: Runtime> RustOnly<R> {
    /// Wraps `plugin`.
    fn new(plugin: impl Plugin<R> + 'static) -> Self {
        RustOnly(Box::new(plugin))
    }
}

// Every method is written out, defaults included, and clippy fails on one that is not: a hook a
// future Tauri adds would otherwise silently skip the wrapped plugin.
#[deny(clippy::missing_trait_methods)]
impl<R: Runtime> Plugin<R> for RustOnly<R> {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn initialize(
        &mut self,
        app: &AppHandle<R>,
        config: serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.0.initialize(app, config)
    }

    fn initialization_script(&self) -> Option<String> {
        None
    }

    fn initialization_script_2(&self) -> Option<InitializationScript> {
        None
    }

    fn window_created(&mut self, window: Window<R>) {
        self.0.window_created(window);
    }

    fn webview_created(&mut self, webview: Webview<R>) {
        self.0.webview_created(webview);
    }

    fn on_navigation(&mut self, webview: &Webview<R>, url: &Url) -> bool {
        self.0.on_navigation(webview, url)
    }

    fn on_page_load(&mut self, webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
        self.0.on_page_load(webview, payload);
    }

    fn on_event(&mut self, app: &AppHandle<R>, event: &RunEvent) {
        self.0.on_event(app, event);
    }

    fn extend_api(&mut self, invoke: Invoke<R>) -> bool {
        self.0.extend_api(invoke)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Wry;

    /// The unwrapped plugin injects a script (so the test means something), and the wrapped one
    /// keeps the plugin's name and injects nothing.
    fn assert_script_dropped<P: Plugin<Wry> + 'static>(name: &str, plugin: P) {
        assert!(
            plugin.initialization_script_2().is_some(),
            "{name} no longer injects a script; update the RustOnly docs"
        );
        let wrapped = RustOnly::new(plugin);
        assert_eq!(Plugin::<Wry>::name(&wrapped), name);
        assert!(Plugin::<Wry>::initialization_script(&wrapped).is_none());
        assert!(Plugin::<Wry>::initialization_script_2(&wrapped).is_none());
    }

    #[test]
    fn wrapped_plugins_keep_their_name_and_drop_their_page_script() {
        assert_script_dropped("notification", tauri_plugin_notification::init());
        assert_script_dropped("dialog", tauri_plugin_dialog::init());
        assert_script_dropped("opener", tauri_plugin_opener::init());
        assert_script_dropped("os", tauri_plugin_os::init());
    }
}
