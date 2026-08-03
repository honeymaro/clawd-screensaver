//! Window and event-loop plumbing for the running screensaver.

use std::time::Duration;

use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy},
    window::{Fullscreen, Window, WindowBuilder},
};
use wry::{WebView, WebViewBuilder, http::Request};

use crate::{Opts, usage};

const UI: &str = include_str!("ui.html");

/// A fetch takes 7-9s on its own, so this is close to the floor: the poller
/// subtracts the fetch time from the wait, which means node is running most of
/// the time the saver is up. Worth it only because it makes a long-running
/// background agent's spend visibly tick upward.
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
  function quit() { if (armed && window.ipc) window.ipc.postMessage('exit'); }
  addEventListener('mousemove', function (e) {
    if (ox === null) { ox = e.screenX; oy = e.screenY; return; }
    if (Math.abs(e.screenX - ox) > 8 || Math.abs(e.screenY - oy) > 8) quit();
  }, true);
  addEventListener('mousedown', quit, true);
  addEventListener('wheel', quit, true);
  addEventListener('keydown', quit, true);
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

pub fn run(opts: &Opts) -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Whatever was last recorded today, so the counter is populated on the very
    // first frame instead of after the ~2s ccusage round trip.
    let seed = usage::initial();

    // Comparing MonitorHandle values directly is awkward; positions are unique
    // per display and compare cleanly.
    let primary_pos = event_loop.primary_monitor().map(|m| m.position());

    let mut surfaces: Vec<(Window, WebView)> = Vec::new();

    if opts.windowed {
        surfaces.push(build_surface(
            WindowBuilder::new()
                .with_title("Clawd Saver")
                .with_inner_size(LogicalSize::new(1280.0, 800.0)),
            true,
            opts,
            &seed,
            &proxy,
            &event_loop,
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
            let (window, webview) =
                build_surface(builder, is_primary, opts, &seed, &proxy, &event_loop)?;
            window.set_cursor_visible(false);
            window.set_visible(true);
            surfaces.push((window, webview));
        }
    }

    if surfaces.is_empty() {
        return Ok(());
    }

    usage::spawn_poller(proxy.clone(), REFRESH);

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
            _ => {}
        }
    });
}

fn build_surface(
    builder: WindowBuilder,
    animated: bool,
    opts: &Opts,
    seed: &usage::Usage,
    proxy: &EventLoopProxy<UserEvent>,
    target: &EventLoop<UserEvent>,
) -> wry::Result<(Window, WebView)> {
    let window = builder.build(target).unwrap();

    // wry replaces the init script rather than appending, so the pieces are
    // concatenated up front.
    let mut init = format!("window.CLAWD_SEED = {};", seed.to_json());
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
    let webview = WebViewBuilder::new()
        .with_html(UI)
        .with_background_color(BG)
        .with_initialization_script(init)
        .with_ipc_handler(move |req: Request<String>| {
            let body = req.body().as_str();
            if body == "exit" {
                let _ = quit_proxy.send_event(UserEvent::Quit("input from page"));
            } else if let Some(payload) = body.strip_prefix("diag:") {
                eprintln!("[saver] diag {payload}");
            }
        })
        .build(&window)?;

    Ok((window, webview))
}
