//! Reads today's Claude Code spend by shelling out to ccusage.

use std::{
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tao::event_loop::EventLoopProxy;

/// Pinned instead of `@latest`. `@latest` re-resolves against the npm registry
/// on every single run, which costs a round trip and fails outright offline.
///
/// `install.ps1` reads this line to decide what to install into the runtime
/// directory, so the bundled copy and the pnpx fallback cannot drift apart.
const CCUSAGE: &str = "ccusage@20.0.19";

/// Stops a console window from flashing onto the screensaver when we shell out.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use crate::saver::UserEvent;

#[derive(Clone, Copy)]
pub enum Freshness {
    /// Straight from a successful ccusage run.
    Fresh,
    /// A run is in flight. Distinct from Failed so the page can show that
    /// something is happening — the two looked identical before, which made the
    /// first launch of each day read as broken for several seconds.
    Loading,
    /// ccusage failed but a previous value was on disk.
    Stale,
    /// ccusage failed and there is nothing cached.
    Failed,
}

#[derive(Clone, Copy)]
pub struct Usage {
    pub cost: Option<f64>,
    pub freshness: Freshness,
}

impl Usage {
    pub fn to_json(self) -> String {
        let state = match self.freshness {
            Freshness::Fresh => "ok",
            Freshness::Loading => "loading",
            Freshness::Stale => "stale",
            Freshness::Failed => "error",
        };
        match self.cost {
            Some(c) => format!("{{\"cost\":{c:.4},\"state\":\"{state}\"}}"),
            None => format!("{{\"cost\":null,\"state\":\"{state}\"}}"),
        }
    }
}

/// Everything this program installs, writes or caches lives here.
fn app_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("clawd-saver"))
}

#[repr(C)]
#[derive(Default)]
struct SystemTimeFields {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

fn local_now() -> SystemTimeFields {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(st: *mut SystemTimeFields);
    }
    let mut st = SystemTimeFields::default();
    unsafe { GetLocalTime(&mut st) };
    st
}

/// Local calendar date as `YYYYMMDD`. ccusage groups by local timezone, so a UTC
/// date would report the wrong day for part of every evening.
fn today() -> String {
    let st = local_now();
    format!("{:04}{:02}{:02}", st.year, st.month, st.day)
}

// ── Runners ──────────────────────────────────────────────────────────────────

/// One way of invoking ccusage.
struct Runner {
    /// Short name for the log. The local runner's real command line is two
    /// absolute paths, which is far too long to read at a glance.
    label: &'static str,
    program: String,
    lead: Vec<String>,
    /// Whether the program has to be launched through `cmd`.
    ///
    /// `Command::new("pnpx")` fails with "program not found" — CreateProcess
    /// appends `.exe` and does not search PATHEXT, so it never finds
    /// `pnpx.CMD`. Naming the extension explicitly would work for the npm
    /// shims, but `cmd` is what lets a globally installed `ccusage` resolve to
    /// whatever extension its installer happened to use.
    ///
    /// The tradeoff: routing through `cmd` skips the batch-argument escaping
    /// the standard library applies when it resolves a `.CMD` itself. Safe only
    /// because every argument below is a fixed string, an installer-recorded
    /// path or a digits-only date. Do not pass anything through here that
    /// originates outside this program.
    ///
    /// The local runner is a real `.exe` at an absolute path, so it needs none
    /// of that and is launched directly.
    via_cmd: bool,
}

/// The copy of ccusage that `install.ps1` puts under the install directory, run
/// through the `node.exe` recorded beside it.
///
/// This is the only runner that consults neither PATH nor a package resolver,
/// and it is first for both reasons. `pnpx` spends nearly all of its time
/// resolving the package rather than running it — 1.1s here against 2-13s, and
/// around 20s on the first run of a day when the dlx cache has to be rebuilt.
/// And a screensaver the system launches can inherit a PATH with no node, no
/// pnpm and no npm on it, in which case every runner below fails within
/// milliseconds and the counter never fills in at all.
fn local_runner() -> Option<Runner> {
    let runtime = app_dir()?.join("runtime");
    let cli = runtime.join("node_modules").join("ccusage").join("src").join("cli.js");
    if !cli.is_file() {
        return None;
    }
    Some(Runner {
        label: "local",
        program: node_exe(&runtime)?,
        lead: vec![cli.to_string_lossy().into_owned()],
        via_cmd: false,
    })
}

/// Where node lives, without consulting PATH. The install script records the
/// absolute path while it still has a full environment; the default installer
/// location is the fallback for a node that has since been moved or reinstalled.
fn node_exe(runtime: &Path) -> Option<String> {
    let recorded = std::fs::read_to_string(runtime.join("node.txt")).ok();
    let default =
        std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("nodejs").join("node.exe"));
    recorded
        // A BOM is not whitespace, so trim() alone would leave one glued to the
        // front of the path and the lookup would fail for no visible reason.
        .map(|s| PathBuf::from(s.trim_start_matches('\u{feff}').trim()))
        .into_iter()
        .chain(default)
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

/// Runners are tried in order and the first one that produces a number wins.
///
/// Everything after the local copy is a fallback: for an install that predates
/// the runtime directory, or one where node has since moved. A globally
/// installed `ccusage` also skips pnpx's package resolution, so it is preferred
/// over pnpx; when absent `cmd` fails within milliseconds and the chain moves
/// on.
fn runners() -> Vec<Runner> {
    let shell = |label: &'static str, program: &str, lead: Vec<String>| Runner {
        label,
        program: program.to_string(),
        lead,
        via_cmd: true,
    };

    let mut list: Vec<Runner> = local_runner().into_iter().collect();
    list.push(shell("ccusage", "ccusage", vec![]));
    list.push(shell("pnpx", "pnpx", vec![CCUSAGE.into()]));
    list.push(shell("npx", "npx", vec!["-y".into(), CCUSAGE.into()]));

    // A screensaver can be launched by the system with a thinner PATH than an
    // interactive shell, so fall back to where pnpm installs itself.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let direct = PathBuf::from(local).join("pnpm").join("pnpx.CMD");
        if direct.is_file() {
            list.push(shell("pnpx.CMD", &direct.to_string_lossy(), vec![CCUSAGE.into()]));
        }
    }
    list
}

/// On success, also reports which runner won — the difference between the local
/// copy and the pnpx fallback is an order of magnitude in wall-clock, so the log
/// is close to useless without it.
pub fn fetch() -> Result<(f64, String), String> {
    let day = today();
    let started = std::time::Instant::now();
    let mut problems = Vec::new();

    for runner in runners() {
        let mut cmd = if runner.via_cmd {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&runner.program);
            c
        } else {
            Command::new(&runner.program)
        };
        cmd.args(&runner.lead)
            .args(["daily", "--json", "--since", &day, "--until", &day])
            .creation_flags(CREATE_NO_WINDOW);

        let label = runner.label;
        match cmd.output() {
            Ok(out) if out.status.success() => match parse_total(&out.stdout) {
                Ok(cost) => {
                    log(&format!(
                        "fetch ok      {:>6.1}s  via {label}  ${cost:.2}",
                        started.elapsed().as_secs_f32()
                    ));
                    return Ok((cost, label.to_string()));
                }
                Err(e) => problems.push(format!("{label}: {e}")),
            },
            Ok(out) => problems.push(format!(
                "{label}: {} - {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim().replace('\n', " ")
            )),
            Err(e) => problems.push(format!("{label}: {e}")),
        }
    }

    let why = problems.join(" | ");
    // The environment is only worth recording when something broke; a thinner
    // PATH than an interactive shell has been the standing suspicion for a
    // process the system launches.
    log(&format!(
        "fetch FAILED  {:>6.1}s\n    {why}\n    PATH={}",
        started.elapsed().as_secs_f32(),
        std::env::var("PATH").unwrap_or_default()
    ));
    Err(why)
}

fn parse_total(stdout: &[u8]) -> Result<f64, String> {
    let v: serde_json::Value =
        serde_json::from_slice(stdout).map_err(|e| format!("unparsable json ({e})"))?;

    // A day with no usage comes back with an empty `daily` array; that is zero
    // spend, not a failure.
    if v.get("daily").and_then(|d| d.as_array()).is_some_and(|a| a.is_empty()) {
        return Ok(0.0);
    }

    v.get("totals")
        .and_then(|t| t.get("totalCost"))
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| "totals.totalCost missing".to_string())
}

// ── Diagnostic log ───────────────────────────────────────────────────────────
//
// A screensaver runs with no console, so a fetch that fails or merely takes
// twenty seconds leaves no trace anywhere. Three separate investigations into
// "it just shows $--.--" each had to start by bolting on temporary logging, so
// this is permanent. One line per fetch: which runner won, how long it took,
// and what came back — that is what each of those investigations actually
// needed.

const LOG_MAX_BYTES: u64 = 64 * 1024;
const LOG_KEEP_LINES: usize = 300;

fn log_path() -> Option<PathBuf> {
    app_dir().map(|d| d.join("log.txt"))
}

pub fn log(msg: &str) {
    use std::io::Write;
    let Some(path) = log_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    // Trim before appending so the file cannot grow without bound. Rewriting a
    // 64 KB file a few times a month costs nothing.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX_BYTES)
        && let Ok(body) = std::fs::read_to_string(&path)
    {
        let lines: Vec<&str> = body.lines().collect();
        let keep = lines.len().saturating_sub(LOG_KEEP_LINES);
        let _ = std::fs::write(&path, lines[keep..].join("\r\n") + "\r\n");
    }

    let t = local_now();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}  {msg}",
            t.year, t.month, t.day, t.hour, t.minute, t.second
        );
    }
}

fn cache_path() -> Option<PathBuf> {
    app_dir().map(|d| d.join("last.json"))
}

/// The cached value is only meaningful for the day it was written; spend resets
/// at midnight, so yesterday's total must not be shown as today's.
pub fn read_cache() -> Option<f64> {
    let raw = std::fs::read_to_string(cache_path()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    if v.get("day").and_then(|d| d.as_str())? != today() {
        return None;
    }
    v.get("cost").and_then(serde_json::Value::as_f64)
}

fn write_cache(cost: f64) {
    let Some(path) = cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let body = format!(
        "{{\"day\":\"{}\",\"cost\":{cost:.4},\"at\":{at}}}",
        today()
    );
    let _ = std::fs::write(path, body);
}

/// What to show before the first ccusage run finishes. Even the local runner
/// takes about a second, and staring at a blank counter for that long is worse
/// than showing the last figure recorded today.
pub fn initial() -> Usage {
    match read_cache() {
        Some(cost) => Usage {
            cost: Some(cost),
            freshness: Freshness::Stale,
        },
        None => Usage {
            cost: None,
            freshness: Freshness::Failed,
        },
    }
}

/// Fetches immediately, then on an interval, pushing each result onto the event
/// loop. Stops as soon as the event loop is gone.
///
/// The wait is the remainder of the interval, not the whole of it, so a fetch
/// that takes several seconds does not stretch the requested cadence by that
/// much on top.
///
/// But the remainder alone is wrong when a fetch outlasts the interval — it
/// comes out zero and node then runs continuously for as long as the saver is
/// up. MIN_IDLE keeps a floor under it.
pub fn spawn_poller(proxy: EventLoopProxy<UserEvent>, interval: Duration) {
    std::thread::spawn(move || {
        loop {
            let started = std::time::Instant::now();

            // Announce the run before making it. A fetch can take several
            // seconds, and on the first launch of a new day there is no cached
            // figure to show meanwhile, so the page needs to know the blank is
            // temporary.
            let mut pending = initial();
            pending.freshness = Freshness::Loading;
            if proxy.send_event(UserEvent::Usage(pending)).is_err() {
                return;
            }

            // fetch() writes its own log line, so every call site is covered.
            let usage = match fetch() {
                Ok((cost, _)) => {
                    write_cache(cost);
                    Usage {
                        cost: Some(cost),
                        freshness: Freshness::Fresh,
                    }
                }
                Err(e) => {
                    eprintln!("[usage] {e}");
                    initial()
                }
            };
            if proxy.send_event(UserEvent::Usage(usage)).is_err() {
                return;
            }
            const MIN_IDLE: Duration = Duration::from_secs(5);
            std::thread::sleep(interval.saturating_sub(started.elapsed()).max(MIN_IDLE));
        }
    });
}

/// Headless check: run the whole pipeline once and report, without opening a
/// window.
pub fn print_once() {
    let started = std::time::Instant::now();
    println!(
        "runners = {:?}",
        runners().iter().map(|r| r.label).collect::<Vec<_>>()
    );
    match fetch() {
        Ok((cost, runner)) => {
            write_cache(cost);
            println!(
                "today ({}) = ${cost:.4}   [{:?} via {runner}]",
                today(),
                started.elapsed()
            );
            println!("cache  = {:?}", cache_path());
            println!("reread = {:?}", read_cache());
            println!("log    = {:?}", log_path());
        }
        Err(e) => println!("FAILED after {:?}\n{e}", started.elapsed()),
    }
}
