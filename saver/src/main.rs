// Clawd Saver — a Windows screensaver (.scr).
// Console is hidden in release only; debug builds keep it so println! is visible.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod saver;
mod usage;

/// How Windows invoked us. The screensaver contract is a small set of switches
/// passed on the command line.
pub enum Mode {
    /// `/s` — run full screen. The real thing.
    Run,
    /// `/c` or `/c:<hwnd>` — show the settings dialog. Also what a bare
    /// double-click means.
    Config,
    /// `/p <hwnd>` — draw into the little preview pane of the settings dialog.
    Preview,
}

pub struct Opts {
    pub mode: Mode,
    /// Dev switch: use an ordinary decorated window instead of taking over
    /// every display.
    pub windowed: bool,
    /// Dev switch: quit on a timer, for unattended checks.
    pub exit_after_ms: Option<u64>,
    /// Dev switch: do not wire up the input-driven exit, so a screenshot can be
    /// taken without the cursor dismissing the saver.
    pub ignore_input: bool,
    /// Dev switch: have the page report its viewport metrics over IPC.
    pub diag: bool,
    /// Dev switch: run the ccusage pipeline once and print, opening no window.
    pub print_usage: bool,
    /// Not a dev switch: how the detached child launched by a running saver
    /// identifies itself. It refreshes the cache and exits, opening no window.
    pub refresh_cache: bool,
}

fn parse_args() -> Opts {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Windows is inconsistent here: the switch may be `/s`, `-s`, `/S`, and the
    // config variant arrives either as `/c` or glued to a window handle as
    // `/c:12345`.
    let mut mode = Mode::Config; // bare invocation (double-click) means config
    let mut windowed = false;
    let mut exit_after_ms = None;
    let mut ignore_input = false;
    let mut diag = false;
    let mut print_usage = false;
    let mut refresh_cache = false;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_ascii_lowercase();
        let switch = arg.trim_start_matches(['/', '-']);
        match switch.chars().next() {
            Some('s') if switch.len() == 1 => mode = Mode::Run,
            Some('c') if switch.len() == 1 || switch.starts_with("c:") => mode = Mode::Config,
            Some('p') if switch.len() == 1 || switch.starts_with("p:") => mode = Mode::Preview,
            _ => match arg.as_str() {
                "--windowed" => {
                    windowed = true;
                    mode = Mode::Run;
                }
                "--ignore-input" => ignore_input = true,
                "--diag" => diag = true,
                "--print-usage" => print_usage = true,
                "--refresh-cache" => refresh_cache = true,
                "--exit-after" => {
                    exit_after_ms = args.get(i + 1).and_then(|v| v.parse().ok());
                    i += 1;
                }
                _ => {}
            },
        }
        i += 1;
    }

    Opts {
        mode,
        windowed,
        exit_after_ms,
        ignore_input,
        diag,
        print_usage,
        refresh_cache,
    }
}

/// Without this the window is DPI-unaware, and WebView2 then reports
/// `innerWidth` in physical pixels while `devicePixelRatio` still claims 1.5.
/// Any canvas sizing that multiplies the two double-counts the scale factor and
/// overflows the screen. Must run before the first window exists.
fn make_dpi_aware() {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetProcessDpiAwarenessContext(value: isize) -> i32;
    }
    const PER_MONITOR_AWARE_V2: isize = -4;
    unsafe { SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2) };
}

fn main() {
    make_dpi_aware();
    let opts = parse_args();

    if opts.print_usage {
        usage::print_once();
        return;
    }

    // Checked before the mode match, and before anything can open a window, so
    // the child can never recurse into launching another child.
    if opts.refresh_cache {
        usage::refresh_cache_once();
        return;
    }

    match opts.mode {
        // Rendering a WebView2 instance into the settings dialog's postage-stamp
        // thumbnail costs far more than it is worth, so the preview stays blank.
        Mode::Preview => {}
        // The interval is read from the constant rather than spelled out, so this
        // dialog cannot drift out of sync with what the poller actually does.
        Mode::Config => message_box(
            "Clawd Saver",
            &format!(
                "Clawd Saver has no settings.\n\nToday's Claude Code spend is read with ccusage \
                 and refreshed every {} seconds while the saver runs.",
                saver::REFRESH.as_secs()
            ),
        ),
        Mode::Run => {
            // Started before the window exists, for both of its reasons: the
            // fetch is already in flight while WebView2 spins up, and it keeps
            // going if this saver is dismissed a second from now — which is the
            // common case on a machine someone is actually using, and the reason
            // the cached figure used to freeze for hours.
            usage::spawn_detached_refresh();
            if let Err(e) = saver::run(&opts) {
                message_box("Clawd Saver", &format!("Failed to start:\n\n{e}"));
            }
        }
    }
}

/// A message box straight from user32, so the `windows` crate stays out of the
/// dependency tree for one dialog.
fn message_box(caption: &str, text: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            utype: u32,
        ) -> i32;
    }

    let wide = |s: &str| {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (t, c) = (wide(text), wide(caption));
    const MB_ICONINFORMATION: u32 = 0x40;
    unsafe { MessageBoxW(std::ptr::null_mut(), t.as_ptr(), c.as_ptr(), MB_ICONINFORMATION) };
}
