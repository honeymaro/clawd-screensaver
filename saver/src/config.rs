//! The settings window: `/c`, `/c:<hwnd>`, or a bare invocation — which is how
//! it is actually reached, since the `config` shell verb passes no arguments.
//!
//! Built on the tao and wry that are already here rather than on a Win32 dialog
//! template. A radio group and two buttons is a lot of `CreateWindowEx` and
//! `WM_COMMAND` plumbing for something the project can already express as a page,
//! and the whole visual side of this program is HTML anyway.
//!
//! The cost is a WebView2 instance, about a second to start. That is the same
//! cost that makes `/p` — the settings-dialog thumbnail, repainted constantly
//! while the list is scrolled — not worth implementing; here it is paid once,
//! after the user has asked for it.

use tao::{
    dpi::{LogicalSize, PhysicalPosition},
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder},
    window::WindowBuilder,
};
use wry::{WebViewBuilder, http::Request};

use crate::{
    settings::{self, Period},
    usage,
};

const UI: &str = include_str!("settings.html");

/// Matches the page background so there is no white flash before first paint.
const BG: wry::RGBA = (20, 18, 15, 255);

#[derive(PartialEq, Eq, Debug)]
enum Msg {
    Save(Period),
    Close,
}

/// What the page asked for.
///
/// Anything that is not a `save:` for a period this build knows about closes
/// without writing. That covers the Cancel button, the window's X, and the case
/// where settings.html and settings.rs have drifted apart — in which case
/// leaving whatever is already stored alone is the safe reading.
fn message(body: &str) -> Msg {
    body.strip_prefix("save:")
        .and_then(Period::from_key)
        .map_or(Msg::Close, Msg::Save)
}

pub fn run(current: Period) -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Clawd Saver")
        // Sized to hold the five options without a scrollbar. The window cannot
        // be resized, so if anything ever outgrows it the page scrolls its own
        // list rather than pushing Save and Cancel off the bottom.
        .with_inner_size(LogicalSize::new(440.0, 430.0))
        .with_resizable(false)
        .with_visible(false) // revealed once it has been placed
        .build(&event_loop)
        .unwrap();

    // Windows places a new window wherever it likes. A settings dialog that
    // opens off in a corner of a 4K display reads as a stray window.
    if let Some(monitor) = window.primary_monitor() {
        let screen = monitor.size();
        let size = window.outer_size();
        let origin = monitor.position();
        window.set_outer_position(PhysicalPosition::new(
            origin.x + (screen.width.saturating_sub(size.width) / 2) as i32,
            origin.y + (screen.height.saturating_sub(size.height) / 2) as i32,
        ));
    }
    window.set_visible(true);
    // A window revealed after creation does not necessarily come up focused, and
    // an unfocused dialog ignores the keyboard entirely — Escape, Enter and the
    // arrow keys the page binds all go nowhere.
    window.set_focus();

    // `key()` is a bare ASCII identifier, so this cannot escape the string
    // literal. It is still written through the JSON serialiser rather than
    // pasted, because "the value is safe today" is not a property that survives
    // someone adding a period later.
    let init = format!(
        "window.CLAWD_PERIOD = {};",
        serde_json::Value::from(current.key())
    );

    let mut web_context = usage::web_context();
    let _webview = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_html(UI)
        .with_background_color(BG)
        .with_initialization_script(init)
        .with_ipc_handler(move |req: Request<String>| {
            let _ = proxy.send_event(message(req.body().as_str()));
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Msg::Save(period)) => {
                match settings::save(period) {
                    Ok(()) => usage::log(&format!("settings      period={}", period.key())),
                    Err(e) => {
                        // Everywhere else in this program the log is the only
                        // channel there is. This is the one moment someone is
                        // sitting in front of a window, and a dialog that
                        // closes on Save without saving is indistinguishable
                        // from one that worked.
                        usage::log(&format!("settings FAILED  {e}"));
                        crate::message_box(
                            "Clawd Saver",
                            &format!("Could not save the setting:\n\n{e}"),
                        );
                    }
                }
                *control_flow = ControlFlow::Exit;
            }
            // `Destroyed` as well as `CloseRequested`, for the same reason the
            // saver handles both: a window that goes away without asking would
            // otherwise leave this process waiting on a message queue forever,
            // invisible and still holding the .scr open against the next
            // install. Cheaper to exit than to explain why it need not.
            Event::UserEvent(Msg::Close)
            | Event::WindowEvent {
                event: WindowEvent::CloseRequested | WindowEvent::Destroyed,
                ..
            } => *control_flow = ControlFlow::Exit,
            _ => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys `settings.html` actually offers, scraped out of the page itself.
    ///
    /// Read rather than repeated because a repeated list is only a guess about
    /// the page: change both together and nothing notices. The page is compiled
    /// into this binary, so the test can consult the real thing.
    fn keys_offered_by_the_page() -> Vec<&'static str> {
        UI.match_indices("key: '")
            .map(|(at, m)| {
                let rest = &UI[at + m.len()..];
                &rest[..rest
                    .find('\'')
                    .expect("unterminated key literal in settings.html")]
            })
            .collect()
    }

    #[test]
    fn every_option_the_page_offers_maps_to_a_period() {
        // An option whose key the host does not accept is silently a second
        // Cancel button: the click posts, `message` shrugs, the dialog closes.
        let offered = keys_offered_by_the_page();
        for key in &offered {
            let period = Period::from_key(key)
                .unwrap_or_else(|| panic!("settings.html offers {key:?}, which no Period accepts"));
            assert_eq!(message(&format!("save:{key}")), Msg::Save(period));
        }
        // And the other direction: a period added to the enum but not to the
        // page would otherwise be unreachable with nothing to say so.
        assert_eq!(
            offered.len(),
            Period::ALL.len(),
            "settings.html offers {offered:?} against {} periods",
            Period::ALL.len()
        );
    }

    #[test]
    fn cancelling_and_closing_do_not_write() {
        for body in ["close", "", "save:", "save:1y", "exit:mousemove", "save:1D"] {
            assert_eq!(message(body), Msg::Close, "{body:?} should not save");
        }
    }
}
