//! Window and event-loop plumbing for the running screensaver.

use std::time::Duration;

use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy},
    window::{Fullscreen, Window, WindowBuilder},
};
use wry::{WebView, WebViewBuilder, http::Request};

use crate::{
    Opts,
    settings::{Period, Scene},
    usage,
};

const UI: &str = include_str!("ui.html");

/// The bundled ccusage answers in about a second from a shell, but the saver's
/// own log has recorded the same call taking up to 6.8s while the page is
/// animating. The PATH-resolved fallbacks are far worse: 25s and 94s have both
/// been logged in a live session. So this is the real cadence only when the fast
/// path is available and a lower bound otherwise — the poller subtracts the
/// fetch from the wait and keeps a floor under it. Polling this often is worth
/// it because it makes a long-running background agent's spend visibly tick
/// upward.
pub const REFRESH: Duration = Duration::from_secs(10);

/// Messages pushed onto the event loop from outside it. The quit reason is
/// carried along so the timer and a genuine input dismissal stay tellable apart
/// in the log.
pub enum UserEvent {
    Quit(&'static str),
    Usage(usage::Usage),
}

/// WebView2 sits on top of the whole window and consumes mouse and keyboard
/// input before tao's event loop ever sees it. A screensaver that cannot detect
/// input is a screensaver that will not close, so the trigger has to live inside
/// the page and travel back over IPC.
const EXIT_SCRIPT: &str = r#"
(function () {
  var armed = false, ox = null, oy = null;
  // Windows sometimes emits a phantom mousemove the instant the saver appears.
  // Stay deaf briefly, then require real cursor travel before quitting.
  setTimeout(function () { armed = true; }, 600);
  function quit(why) { if (armed && window.ipc) window.ipc.postMessage('exit:' + why); }
  addEventListener('mousemove', function (e) {
    if (ox === null) { ox = e.screenX; oy = e.screenY; return; }
    var dx = Math.abs(e.screenX - ox), dy = Math.abs(e.screenY - oy);
    if (dx > 8 || dy > 8) quit('mousemove d=' + dx + ',' + dy);
  }, true);
  addEventListener('mousedown', function () { quit('mousedown'); }, true);
  addEventListener('wheel', function () { quit('wheel'); }, true);
  addEventListener('keydown', function (e) { quit('keydown ' + e.key); }, true);
  addEventListener('contextmenu', function (e) { e.preventDefault(); }, true);
})();
"#;

/// Secondary displays get the same page with the animation switched off, which
/// avoids running one renderer per monitor at full tilt.
const STATIC_FLAG: &str = "window.CLAWD_STATIC = true;";

/// Reports the viewport numbers the page actually sees, so canvas scaling can be
/// debugged against real values instead of assumptions about DPI.
const DIAG_SCRIPT: &str = r#"
setTimeout(function () {
  var c = document.getElementById('stage');
  var s = getComputedStyle(c);
  window.ipc.postMessage('diag:' + JSON.stringify({
    innerW: innerWidth, innerH: innerHeight, dpr: devicePixelRatio,
    backingW: c.width, backingH: c.height,
    cssW: s.width, cssH: s.height,
    screenW: screen.width, screenH: screen.height
  }));
}, 1500);
"#;

/// Matches the page background so there is no white flash before first paint.
const BG: wry::RGBA = (26, 24, 22, 255);

pub fn run(opts: &Opts, period: Period, scene: Scene) -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Whatever was last recorded today for this period, so the counter is
    // populated on the very first frame instead of after the ~2s ccusage round
    // trip.
    let seed = usage::initial(period);

    // Comparing MonitorHandle values directly is awkward; positions are unique
    // per display and compare cleanly.
    let primary_pos = event_loop.primary_monitor().map(|m| m.position());

    let mut surfaces: Vec<(Window, WebView)> = Vec::new();
    // Shared by every display's webview, and by the settings dialog in its own
    // process. See usage::web_context for what happens without it.
    let mut web_context = usage::web_context();

    if opts.windowed {
        surfaces.push(build_surface(
            WindowBuilder::new()
                .with_title("Clawd Saver")
                .with_inner_size(LogicalSize::new(1280.0, 800.0)),
            Page { animated: true, opts, seed: &seed, scene },
            &proxy,
            &event_loop,
            &mut web_context,
        )?);
    } else {
        for monitor in event_loop.available_monitors() {
            let is_primary = Some(monitor.position()) == primary_pos;
            let builder = WindowBuilder::new()
                .with_title("Clawd Saver")
                .with_decorations(false)
                .with_always_on_top(true)
                .with_visible(false) // revealed once the webview exists
                .with_fullscreen(Some(Fullscreen::Borderless(Some(monitor))));
            let (window, webview) = build_surface(
                builder,
                Page { animated: is_primary, opts, seed: &seed, scene },
                &proxy,
                &event_loop,
                &mut web_context,
            )?;
            window.set_cursor_visible(false);
            window.set_visible(true);
            surfaces.push((window, webview));
        }
    }

    if surfaces.is_empty() {
        return Ok(());
    }

    // One line per session. Paired with the fetch lines it answers the two
    // questions that have cost the most time: did the saver start at all, and
    // did it have a same-day figure to show before the first fetch landed.
    usage::log(&format!(
        "saver start   {} display(s), {}, {}, seed={}",
        surfaces.len(),
        period.key(),
        scene.key(),
        match seed.cost {
            Some(c) => format!("${c:.2} cached"),
            None => "none".into(),
        }
    ));

    usage::spawn_poller(proxy.clone(), REFRESH, period);

    if let Some(ms) = opts.exit_after_ms {
        let proxy = proxy.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            let _ = proxy.send_event(UserEvent::Quit("exit-after timer"));
        });
    }

    // Deliberately no exit-on-focus-loss. It is not part of the screensaver
    // contract, and it self-destructs on multi-monitor setups: creating the
    // second window blurs the first, which would quit before anyone sees it.
    // Input arrives over IPC from the page instead.
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Quit(why)) => {
                eprintln!("[saver] quit: {why}");
                *control_flow = ControlFlow::Exit
            }
            Event::UserEvent(UserEvent::Usage(u)) => {
                eprintln!("[saver] usage {}", u.to_json());
                let js = format!("window.CLAWD_USAGE && window.CLAWD_USAGE({});", u.to_json());
                for (_, webview) in &surfaces {
                    let _ = webview.evaluate_script(&js);
                }
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                eprintln!("[saver] quit: close requested");
                *control_flow = ControlFlow::Exit
            }
            // A window can go away without asking first. Handling only
            // CloseRequested left the event loop running with nothing on
            // screen, and the poller runs off the event loop rather than off
            // the window — so the process became an invisible thing shelling
            // out to ccusage every ten seconds, holding a handle on the .scr
            // that blocks reinstalling it. Observed lasting twenty minutes.
            Event::WindowEvent {
                event: WindowEvent::Destroyed,
                window_id,
                ..
            } => {
                // The id is logged because on a multi-monitor setup the useful
                // question afterwards is which display went away.
                usage::log(&format!(
                    "saver quit    surface {window_id:?} was destroyed"
                ));
                *control_flow = ControlFlow::Exit
            }
            _ => {}
        }
    });
}

/// Everything a surface's page needs that is not the window itself.
struct Page<'a> {
    /// Off for secondary displays, which paint one frame and stop.
    animated: bool,
    opts: &'a Opts,
    seed: &'a usage::Usage,
    scene: Scene,
}

/// The two values every surface's page is handed before it runs.
///
/// The scene arrives already resolved, and the same one goes to every display. A
/// page that rolled its own would put a different scene on each monitor of a
/// multi-head setup.
///
/// Split out from `build_surface` so a test can read it. Both halves of this are
/// a contract with `ui.html` that nothing else enforces: the page looks the
/// scene up in a registry and quietly falls back to the mine for a name it does
/// not know, so a key or a global renamed on one side only is not an error
/// anybody sees — it is the wrong scene, with the log still naming the right one.
fn globals(seed: &usage::Usage, scene: Scene) -> String {
    format!(
        "window.CLAWD_SEED = {};window.CLAWD_SCENE = {};",
        seed.to_json(),
        serde_json::Value::from(scene.key())
    )
}

fn build_surface(
    builder: WindowBuilder,
    page: Page,
    proxy: &EventLoopProxy<UserEvent>,
    target: &EventLoop<UserEvent>,
    web_context: &mut wry::WebContext,
) -> wry::Result<(Window, WebView)> {
    let Page {
        animated,
        opts,
        seed,
        scene,
    } = page;
    let window = builder.build(target).unwrap();

    // wry replaces the init script rather than appending, so the pieces are
    // concatenated up front.
    let mut init = globals(seed, scene);
    if !animated {
        init.push_str(STATIC_FLAG);
    }
    if !opts.ignore_input {
        init.push_str(EXIT_SCRIPT);
    }
    if opts.diag {
        init.push_str(DIAG_SCRIPT);
    }

    let quit_proxy = proxy.clone();
    let webview = WebViewBuilder::new_with_web_context(web_context)
        .with_html(UI)
        .with_background_color(BG)
        .with_initialization_script(init)
        .with_ipc_handler(move |req: Request<String>| {
            let body = req.body().as_str();
            if body.starts_with("exit") {
                let _ = quit_proxy.send_event(UserEvent::Quit("input from page"));
            } else if let Some(payload) = body.strip_prefix("diag:") {
                eprintln!("[saver] diag {payload}");
            }
        })
        .build(&window)?;

    Ok((window, webview))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys `ui.html`'s scene registry is indexed by, scraped from the page.
    ///
    /// `checks/verify-scenes.js` cannot do this job. It takes its scene list
    /// from that same registry, so it agrees with the page by construction on
    /// exactly the name that has to match here. The page is compiled into this
    /// binary, so this side can consult the real thing.
    fn registry_keys() -> Vec<&'static str> {
        let at = UI
            .find("const SCENES = {")
            .expect("ui.html no longer declares a SCENES registry");
        let open = at + UI[at..].find('{').expect("no opening brace");
        let mut depth = 0usize;
        let end = UI[open..]
            .char_indices()
            .find_map(|(i, c)| {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
                (depth == 0 && c == '}').then_some(open + i)
            })
            .expect("ui.html's SCENES registry is unterminated");

        UI[open + 1..end]
            .lines()
            .filter_map(|line| line.split_once(':'))
            .map(|(key, _)| key.trim())
            .filter(|key| !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase()))
            .collect()
    }

    #[test]
    fn every_scene_the_host_can_pick_is_one_the_page_can_draw() {
        let keys = registry_keys();
        for scene in Scene::ALL {
            assert!(
                keys.contains(&scene.key()),
                "ui.html draws no scene {:?}; its registry offers {keys:?}",
                scene.key()
            );
        }
        // And the other direction: a scene in the page that no setting can
        // reach is dead weight nobody would find.
        assert_eq!(keys.len(), Scene::ALL.len(), "ui.html offers {keys:?}");
    }

    #[test]
    fn the_page_reads_the_globals_this_file_writes() {
        let seed = usage::Usage {
            cost: Some(12.34),
            freshness: usage::Freshness::Fresh,
        };
        for js in [globals(&seed, Scene::Mine), STATIC_FLAG.to_string()] {
            for name in js.split("window.").skip(1) {
                let name = name.split([' ', '=', ';']).next().unwrap();
                assert!(
                    UI.contains(&format!("window.{name}")),
                    "this file sets window.{name}, which ui.html never reads"
                );
            }
        }
    }
}
