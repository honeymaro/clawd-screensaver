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
    settings::{self, Period, SceneChoice, Settings},
    usage,
};

const UI: &str = include_str!("settings.html");

/// Matches the page background so there is no white flash before first paint.
const BG: wry::RGBA = (20, 18, 15, 255);

#[derive(PartialEq, Eq, Debug)]
enum Msg {
    Save(Settings),
    Close,
}

/// What the page asked for.
///
/// Anything that is not a `save:` carrying values this build knows about closes
/// without writing. That covers the Cancel button, the window's X, and the case
/// where settings.html and settings.rs have drifted apart — in which case
/// leaving whatever is already stored alone is the safe reading.
///
/// A field the page omits, or names something this build does not know, falls
/// back to **what is currently stored** rather than to the default. That is the
/// difference between a dialog that leaves a setting alone and one that resets
/// it: an older page against a newer host would otherwise wipe the half it does
/// not know about, and the user would have no way to tell that from having
/// chosen the default themselves.
///
/// Not reachable today — the page is compiled in and always sends both fields —
/// which is exactly why it is worth pinning before a third field exists.
fn message(body: &str, current: Settings) -> Msg {
    let Some(json) = body.strip_prefix("save:") else {
        return Msg::Close;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return Msg::Close;
    };
    // Valid JSON of the wrong shape is not a save. An array has no fields, so
    // without this it would read as "every field absent" and rewrite the file
    // with what it already said, on a message that meant nothing.
    if !v.is_object() {
        return Msg::Close;
    }
    let field = |name: &str| v.get(name).and_then(serde_json::Value::as_str);
    Msg::Save(Settings {
        period: field("period")
            .and_then(Period::from_key)
            .unwrap_or(current.period),
        scene: field("scene")
            .and_then(SceneChoice::from_key)
            .unwrap_or(current.scene),
    })
}

pub fn run(current: Settings) -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("Clawd Saver")
        // Fifteen options no longer fit, so the page scrolls its own form and
        // keeps Save and Cancel below it. Not made taller to suit: the window
        // cannot be resized, and one sized for the scene list would hang off the
        // bottom of a 768-line laptop screen, which is worse than scrolling.
        .with_inner_size(LogicalSize::new(440.0, 700.0))
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

    // Both `key()`s are bare ASCII identifiers, so this cannot escape the string
    // literals. They are still written through the JSON serialiser rather than
    // pasted, because "the value is safe today" is not a property that survives
    // someone adding a period later.
    let init = format!(
        "window.CLAWD_PERIOD = {};window.CLAWD_SCENE = {};",
        serde_json::Value::from(current.period.key()),
        serde_json::Value::from(current.scene.key())
    );

    let mut web_context = usage::web_context();
    let _webview = WebViewBuilder::new_with_web_context(&mut web_context)
        .with_html(UI)
        .with_background_color(BG)
        .with_initialization_script(init)
        .with_ipc_handler(move |req: Request<String>| {
            let _ = proxy.send_event(message(req.body().as_str(), current));
        })
        .build(&window)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(Msg::Save(chosen)) => {
                match settings::save(chosen) {
                    Ok(()) => usage::log(&format!(
                        "settings      period={} scene={}",
                        chosen.period.key(),
                        chosen.scene.key()
                    )),
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

    /// The keys one of `settings.html`'s lists offers, scraped from the page.
    ///
    /// Read rather than repeated because a repeated list is only a guess about
    /// the page: change both together and nothing notices. The page is compiled
    /// into this binary, so the test can consult the real thing.
    fn keys_in(list: &str) -> Vec<&'static str> {
        let at = UI
            .find(&format!("const {list} = ["))
            .unwrap_or_else(|| panic!("settings.html no longer declares {list}"));
        let body = &UI[at..at + UI[at..].find("];").expect("unterminated list")];
        body.match_indices("key: '")
            .map(|(i, m)| {
                let rest = &body[i + m.len()..];
                &rest[..rest.find('\'').expect("unterminated key literal")]
            })
            .collect()
    }

    /// Something other than the defaults, so a test cannot pass by falling back.
    fn stored() -> Settings {
        Settings {
            period: Period::Last30Days,
            scene: SceneChoice::One(settings::Scene::Rack),
        }
    }

    #[test]
    fn every_period_the_page_offers_is_one_the_host_accepts() {
        // An option whose key the host does not accept is silently a second
        // Cancel button: the click posts, `message` shrugs, the dialog closes.
        let offered = keys_in("PERIODS");
        for key in &offered {
            let period = Period::from_key(key)
                .unwrap_or_else(|| panic!("settings.html offers period {key:?}, unknown here"));
            let body = format!(r#"save:{{"period":"{key}"}}"#);
            let want = Settings { period, ..stored() };
            assert_eq!(message(&body, stored()), Msg::Save(want));
        }
        // And the other direction: a period added to the enum but not to the
        // page would otherwise be unreachable with nothing to say so.
        assert_eq!(offered.len(), Period::ALL.len(), "page offers {offered:?}");
    }

    #[test]
    fn every_scene_the_page_offers_is_one_the_host_accepts() {
        let offered = keys_in("SCENES");
        for key in &offered {
            let scene = SceneChoice::from_key(key)
                .unwrap_or_else(|| panic!("settings.html offers scene {key:?}, unknown here"));
            let body = format!(r#"save:{{"scene":"{key}"}}"#);
            let want = Settings { scene, ..stored() };
            assert_eq!(message(&body, stored()), Msg::Save(want));
        }
        // Deduplicated first: a page listing one scene twice and another not at
        // all has the right number of valid keys and would otherwise pass.
        let distinct: std::collections::HashSet<_> = offered.iter().collect();
        assert_eq!(distinct.len(), offered.len(), "duplicate key in {offered:?}");
        assert_eq!(
            offered.len(),
            SceneChoice::ALL.len(),
            "page offers {offered:?}"
        );
    }

    #[test]
    fn both_fields_travel_together() {
        let body = r#"save:{"period":"30d","scene":"dock"}"#;
        assert_eq!(
            message(body, Settings::default()),
            Msg::Save(Settings {
                period: Period::Last30Days,
                scene: SceneChoice::One(settings::Scene::Dock),
            })
        );
    }

    #[test]
    fn a_field_the_page_did_not_send_leaves_what_is_stored_alone() {
        // An older page against a newer host: the half it knows still applies,
        // and the half it does not know keeps its value rather than reverting.
        assert_eq!(
            message(r#"save:{"period":"7d"}"#, stored()),
            Msg::Save(Settings {
                period: Period::Last7Days,
                scene: stored().scene
            })
        );
        assert_eq!(message("save:{}", stored()), Msg::Save(stored()));
    }

    #[test]
    fn a_value_this_build_does_not_know_leaves_that_field_alone() {
        // A newer page offering a fifth scene, saved against this host. The
        // scene it named cannot be honoured; overwriting the one that is stored
        // with a default would be the worst of the three options.
        assert_eq!(
            message(r#"save:{"period":"wtd","scene":"volcano"}"#, stored()),
            Msg::Save(Settings {
                period: Period::WeekToDate,
                scene: stored().scene
            })
        );
    }

    #[test]
    fn cancelling_and_closing_do_not_write() {
        for body in [
            "close",
            "",
            "save:",
            "save:1d", // the pre-JSON shape
            "save:not json",
            "save:[]",
            "save:\"1d\"",
            "exit:mousemove",
        ] {
            assert_eq!(
                message(body, stored()),
                Msg::Close,
                "{body:?} should not save"
            );
        }
    }
}
