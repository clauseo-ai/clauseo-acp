//! `WebView2`'s `ProcessFailed`: a render process that exited is a renderer gone, with
//! `WebView2`'s reason; every other kind (the browser, the GPU, a frame's renderer, an unresponsive
//! renderer still running) is not, and `TERMINATED` (ended from outside) is not a crash.

#![expect(unsafe_code, reason = "WebView2's COM calls")]

use clauseo_counts::schema::RendererGoneReason;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    COREWEBVIEW2_PROCESS_FAILED_REASON, COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED,
    COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED,
    COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY,
    COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED, COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE,
    ICoreWebView2Controller, ICoreWebView2ProcessFailedEventArgs,
    ICoreWebView2ProcessFailedEventArgs2,
};
use webview2_com::ProcessFailedEventHandler;
use windows::core::Interface as _;

/// Calls `gone` for each render process of `controller`'s webview that exits, with its reason.
pub(super) fn watch(
    controller: &ICoreWebView2Controller,
    gone: impl Fn(RendererGoneReason) + 'static,
) -> windows::core::Result<()> {
    // SAFETY: a live controller from Tauri, on the thread it was made on (`with_webview`).
    let webview = unsafe { controller.CoreWebView2() }?;
    let handler = ProcessFailedEventHandler::create(Box::new(move |_sender, args| {
        if let Some(reason) = args.as_ref().and_then(reason_of) {
            gone(reason);
        }
        Ok(())
    }));
    let mut token = 0_i64;
    // SAFETY: the handler is a COM object WebView2 holds a reference to; the token is ours.
    unsafe { webview.add_ProcessFailed(&handler, &raw mut token) }
}

/// The reason, for a render process that exited; `None` for any other failure and for an end
/// from outside.
fn reason_of(args: &ICoreWebView2ProcessFailedEventArgs) -> Option<RendererGoneReason> {
    let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
    // SAFETY: valid event arguments from WebView2, during its event.
    unsafe { args.ProcessFailedKind(&raw mut kind) }.ok()?;
    if kind != COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
        return None;
    }
    // An older runtime has no reason.
    let reason = args
        .cast::<ICoreWebView2ProcessFailedEventArgs2>()
        .ok()
        .and_then(|args2| {
            let mut reason = COREWEBVIEW2_PROCESS_FAILED_REASON::default();
            // SAFETY: as above.
            unsafe { args2.Reason(&raw mut reason) }
                .ok()
                .map(|()| reason)
        });
    gone_reason(kind, reason)
}

/// What a `ProcessFailed` of `kind`, with `reason` when the runtime gave one, means: a renderer
/// gone and why, or nothing.
fn gone_reason(
    kind: COREWEBVIEW2_PROCESS_FAILED_KIND,
    reason: Option<COREWEBVIEW2_PROCESS_FAILED_REASON>,
) -> Option<RendererGoneReason> {
    if kind != COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
        return None;
    }
    Some(match reason {
        Some(COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED) => return None,
        Some(COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED) => RendererGoneReason::Crashed,
        Some(COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY) => RendererGoneReason::OutOfMemory,
        Some(COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE) => RendererGoneReason::Unresponsive,
        Some(COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED) => RendererGoneReason::LaunchFailed,
        _ => RendererGoneReason::Other,
    })
}

#[cfg(test)]
mod tests {
    use clauseo_counts::schema::RendererGoneReason;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
        COREWEBVIEW2_PROCESS_FAILED_REASON, COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY,
        COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNEXPECTED,
        COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE,
    };

    use super::gone_reason;

    const EXITED: COREWEBVIEW2_PROCESS_FAILED_KIND =
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED;

    #[test]
    fn a_render_process_that_exited_is_gone_with_its_reason_and_terminated_is_not() {
        let cases: [(Option<COREWEBVIEW2_PROCESS_FAILED_REASON>, _); 7] = [
            (
                Some(COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED),
                Some(RendererGoneReason::Crashed),
            ),
            (
                Some(COREWEBVIEW2_PROCESS_FAILED_REASON_OUT_OF_MEMORY),
                Some(RendererGoneReason::OutOfMemory),
            ),
            (
                Some(COREWEBVIEW2_PROCESS_FAILED_REASON_UNRESPONSIVE),
                Some(RendererGoneReason::Unresponsive),
            ),
            (
                Some(COREWEBVIEW2_PROCESS_FAILED_REASON_LAUNCH_FAILED),
                Some(RendererGoneReason::LaunchFailed),
            ),
            (
                Some(COREWEBVIEW2_PROCESS_FAILED_REASON_UNEXPECTED),
                Some(RendererGoneReason::Other),
            ),
            // An older runtime gives no reason.
            (None, Some(RendererGoneReason::Other)),
            // Ended from outside: not a crash.
            (Some(COREWEBVIEW2_PROCESS_FAILED_REASON_TERMINATED), None),
        ];
        for (reason, want) in cases {
            assert_eq!(gone_reason(EXITED, reason), want, "{reason:?}");
        }
    }

    #[test]
    fn no_other_kind_of_failure_is_a_renderer_gone() {
        for kind in [
            COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
        ] {
            for reason in [None, Some(COREWEBVIEW2_PROCESS_FAILED_REASON_CRASHED)] {
                assert_eq!(gone_reason(kind, reason), None, "{kind:?} {reason:?}");
            }
        }
    }
}
