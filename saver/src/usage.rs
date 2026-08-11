//! Getting a figure onto the screen, and everything that turned out to involve.
//!
//! Nominally this shells out to ccusage. In practice the shelling out is the
//! small part: which of five runners can be reached, what date window to ask
//! for, where to keep the answer so the next launch has something to show
//! before its own fetch lands, when to refresh it from a process that outlives
//! the saver, and how often to poll without competing with itself. The section
//! banners below are the seams.
//!
//! It also holds the program's directory and its diagnostic log, which the
//! other modules reach for — those sit here because this is where they were
//! first needed, not because they belong to reading spend.

use std::{
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
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

/// Cuts the child loose from this process, so it keeps running after the saver
/// that launched it is gone. The whole point of the refresher below.
const DETACHED_PROCESS: u32 = 0x0000_0008;

use crate::{saver::UserEvent, settings::Period};

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
pub(crate) fn app_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("clawd-saver"))
}

/// The one WebView2 profile every launch shares.
///
/// Left to itself, wry derives the folder from the running module's path — and
/// Windows does not spell that path the same way every time. Both
/// `clawd-saver.scr.WebView2` and `CLAWD-~1.SCR.WebView2` were found side by
/// side, the long and 8.3-short spellings of one binary, holding two profiles of
/// the same cached page: 70 MB and 74 MB on the development machine, both still
/// growing. Which launch produced which was never established, and does not
/// matter — naming the directory outright makes every entry point converge.
///
/// The directory is created here rather than left to WebView2. It does make its
/// own user-data folder, but whether it creates missing *parents* is not
/// something Microsoft's documentation promises, and this is the first thing a
/// launch touches — before `log()` or the cache have had a chance to make
/// `clawd-saver` itself. An installed copy never notices, because `install.ps1`
/// makes that directory first; a binary run straight out of `target` on a
/// machine that has never installed it would be relying on the undocumented
/// half.
pub(crate) fn web_context() -> wry::WebContext {
    let dir = app_dir().map(|d| d.join("webview2"));
    if let Some(d) = &dir {
        let _ = std::fs::create_dir_all(d);
    }
    wry::WebContext::new(dir)
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

// ── Date arithmetic ──────────────────────────────────────────────────────────
//
// Howard Hinnant's civil-date conversions, written out rather than pulled in. A
// date crate would be a dependency for two functions in a project that already
// calls `GetLocalTime` through a hand-written `extern` block for the same
// reason. Both are exact for every date this program can encounter, including
// leap years and century rules.

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    // March-based years, so the leap day lands at the end and needs no case.
    let y = i64::from(y) - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    ((yoe + era * 400 + i64::from(m <= 2)) as i32, m, d)
}

/// Which day the user's Windows locale starts a week on, numbered 0 = Sunday to
/// match what `GetLocalTime` reports.
///
/// Windows answers on a different scale — 0 = Monday through 6 = Sunday — hence
/// the shift. Asking at all is the point: there is no universal answer, ko-KR
/// and en-US start a week on Sunday while most of Europe starts it on Monday,
/// and "this week" is wrong by up to six days if the guess is wrong. A locale
/// that will not answer falls back to Monday, the ISO week.
fn first_day_of_week() -> u32 {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocaleInfoEx(locale: *const u16, lctype: u32, data: *mut u16, size: i32) -> i32;
    }
    // LOCALE_NAME_USER_DEFAULT is a null pointer, not a string.
    const LOCALE_IFIRSTDAYOFWEEK: u32 = 0x0000_100C;
    let mut buf = [0u16; 8];
    let written = unsafe {
        GetLocaleInfoEx(
            std::ptr::null(),
            LOCALE_IFIRSTDAYOFWEEK,
            buf.as_mut_ptr(),
            buf.len() as i32,
        )
    };
    parse_first_day(&buf, written)
}

/// The answer-shaped half of `first_day_of_week`, split out because it is the
/// half that can be wrong quietly.
///
/// `written` is what `GetLocaleInfoEx` returns: characters written *including*
/// the terminator, or zero on failure. The digit is Monday-based; everything
/// else here counts from Sunday, hence the shift. Anything unexpected — a
/// failure, an empty answer, a non-digit, a value outside 0..=6 — falls back to
/// Monday, the ISO week.
fn parse_first_day(buf: &[u16], written: i32) -> u32 {
    let monday_based = (written > 1)
        .then(|| char::from_u32(u32::from(buf[0])).and_then(|c| c.to_digit(10)))
        .flatten()
        .filter(|v| *v <= 6)
        .unwrap_or(0);
    (monday_based + 1) % 7
}

/// Where a period's window starts, given the civil date it ends on.
///
/// `dow` is that day's weekday and `week_starts_on` the locale's first day, both
/// numbered 0 = Sunday. Taking them as arguments rather than reading the clock
/// and the locale is what makes every case below testable.
fn window_start(
    period: Period,
    y: i32,
    m: u32,
    d: u32,
    dow: u32,
    week_starts_on: u32,
) -> (i32, u32, u32) {
    let back = |days: i64| civil_from_days(days_from_civil(y, m, d) - days);
    match period {
        Period::Today => (y, m, d),
        Period::Last7Days => back(6),
        Period::Last30Days => back(29),
        // Distance back to the most recent first-day-of-week, which is zero when
        // today already is that day. The modulo is what keeps it correct whether
        // the locale starts weeks before or after today's weekday.
        Period::WeekToDate => back(i64::from((dow + 7 - week_starts_on) % 7)),
        Period::MonthToDate => (y, m, 1),
    }
}

/// The `--since`/`--until` pair in the `YYYYMMDD` form ccusage takes. Both ends
/// are inclusive, so a one-day window is a single date rather than an empty
/// range.
fn window_ending(
    period: Period,
    y: i32,
    m: u32,
    d: u32,
    dow: u32,
    week_starts_on: u32,
) -> (String, String) {
    let (sy, sm, sd) = window_start(period, y, m, d, dow, week_starts_on);
    (
        format!("{sy:04}{sm:02}{sd:02}"),
        format!("{y:04}{m:02}{d:02}"),
    )
}

/// The window to ask ccusage for, ending today in local time.
fn window(period: Period) -> (String, String) {
    let st = local_now();
    window_ending(
        period,
        i32::from(st.year),
        u32::from(st.month),
        u32::from(st.day),
        u32::from(st.day_of_week),
        first_day_of_week(),
    )
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
        program: node_exe(&runtime),
        lead: vec![cli.to_string_lossy().into_owned()],
        via_cmd: false,
    })
}

/// Where node lives. The install script records the absolute path while it still
/// has a full environment, and the default installer location covers a node that
/// has since been reinstalled.
///
/// The last resort is the bare name, letting CreateProcess search PATH. Unlike
/// the `.CMD` shims that need `cmd`, this works directly, because node really is
/// `node.exe`. It matters for anyone whose node is managed by nvm, fnm or Volta:
/// their recorded path goes stale every time they switch versions and
/// `%ProgramFiles%` never had a node in it, so without this the bundled ccusage
/// would drop out of the chain permanently and silently. If PATH has no node
/// either, the spawn fails immediately and the chain moves on, which is exactly
/// what would have happened anyway.
fn node_exe(runtime: &Path) -> String {
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
        .map_or_else(|| "node".to_string(), |p| p.to_string_lossy().into_owned())
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

    // Where pnpm installs itself, for a PATH that has lost the shim but not
    // node. It cannot rescue a PATH that has lost both: the shim is a batch file
    // whose first act is to invoke `node`, so with `PATH=System32;Windows` it
    // gets as far as running and then reports `'node' is not recognized`. The
    // bundled runner above is the one that survives that case.
    //
    // Launched directly rather than through `cmd`, unlike the three above. They
    // need `cmd` to resolve a bare name against PATH and PATHEXT; this one
    // already names the file including its extension, so `cmd` would add
    // nothing — except the one thing the note on `via_cmd` warns against, since
    // this is the only runner whose command line is built from outside this
    // program. The standard library escapes arguments to a `.CMD` properly;
    // hand-assembling a `cmd /C` line does not.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let direct = PathBuf::from(local).join("pnpm").join("pnpx.CMD");
        if direct.is_file() {
            list.push(Runner {
                label: "pnpx.CMD",
                program: direct.to_string_lossy().into_owned(),
                lead: vec![CCUSAGE.into()],
                via_cmd: false,
            });
        }
    }
    list
}

/// What a successful run produced.
pub struct Fetched {
    pub cost: f64,
    /// Which runner won. The difference between the bundled copy and the pnpx
    /// fallback is an order of magnitude in wall-clock, so the log is close to
    /// useless without it.
    pub runner: &'static str,
    /// The day the window ended on, taken before the run rather than read again
    /// after it.
    ///
    /// This is the whole reason the day is carried out of here. A fetch has been
    /// measured at over three minutes and a screensaver is typically up all
    /// night, so one crossing midnight is close to a nightly event — and
    /// stamping its result with the day it *finished* files yesterday's total
    /// under today, which is exactly what the cache's day key exists to prevent.
    pub day: String,
}

/// Runs ccusage over the period's window and parses the total.
///
/// `tag` is appended to the log line to say who asked. Fetches now come from two
/// places that behave very differently — the poller inside a live saver and the
/// detached refresher that outlives it — and a log that cannot tell them apart
/// makes "why is ccusage running with nothing on screen" unanswerable.
pub fn fetch(tag: &str, period: Period) -> Result<Fetched, String> {
    let (since, until) = window(period);
    let started = std::time::Instant::now();
    let mut problems = Vec::new();
    // Tracked separately from `problems` so a successful fetch can say which
    // runners it had to step over first. Without it a log line reading "via
    // npx" cannot distinguish a bundled copy that was never installed from one
    // that is installed and broken, and only the second is worth reinstalling.
    let mut stepped_over: Vec<&'static str> = Vec::new();

    for runner in runners() {
        let mut cmd = if runner.via_cmd {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&runner.program);
            c
        } else {
            Command::new(&runner.program)
        };
        cmd.args(&runner.lead)
            .args(["daily", "--json", "--since", &since, "--until", &until])
            .creation_flags(CREATE_NO_WINDOW);

        let label = runner.label;
        match cmd.output() {
            Ok(out) if out.status.success() => match parse_total(&out.stdout) {
                Ok(cost) => {
                    let after = if stepped_over.is_empty() {
                        String::new()
                    } else {
                        format!("   (after {})", stepped_over.join(", "))
                    };
                    log(&format!(
                        "fetch ok      {:>6.1}s  via {label}  {:>3}  ${cost:.2}{after}{tag}",
                        started.elapsed().as_secs_f32(),
                        period.key()
                    ));
                    return Ok(Fetched {
                        cost,
                        runner: label,
                        day: until,
                    });
                }
                Err(e) => problems.push(format!("{label}: {e}")),
            },
            Ok(out) => problems.push(format!(
                "{label}: {} - {}",
                out.status,
                // Both, not just the newline: a bare carriage return would fake
                // a line break in anything that renders the log, even though
                // the trimmer's own `lines()` only splits on `\n`.
                String::from_utf8_lossy(&out.stderr)
                    .trim()
                    .replace(['\n', '\r'], " ")
            )),
            Err(e) => problems.push(format!("{label}: {e}")),
        }
        stepped_over.push(label);
    }

    let why = problems.join(" | ");
    // The environment is only worth recording when something broke; a thinner
    // PATH than an interactive shell has been the standing suspicion for a
    // process the system launches.
    log(&format!(
        "fetch FAILED  {:>6.1}s  {}..{}{tag}\n    {why}\n    PATH={}",
        started.elapsed().as_secs_f32(),
        since,
        until,
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

// ── Cache ────────────────────────────────────────────────────────────────────
//
// What the counter shows before a fetch lands. A ccusage run takes seconds, and
// a screensaver on a machine in use is often dismissed before one finishes, so
// the figure on screen usually comes from here rather than from a live fetch.

/// One file per period rather than one slot for whichever period wrote last.
///
/// A single slot let a write for one period destroy a good entry for another.
/// The refresher launched by a saver keeps running after that saver is
/// dismissed, so switching the setting and reopening could have the abandoned
/// refresher land afterwards and overwrite the new period's figure — and the
/// next poll, finding a record for the wrong period, would blank the counter to
/// `$--.--` while a perfectly good number had just been on screen.
///
/// It also means switching back to a period already fetched today shows its
/// figure immediately instead of starting from nothing.
fn cache_path(period: Period) -> Option<PathBuf> {
    app_dir().map(|d| d.join(format!("last-{}.json", period.key())))
}

/// Whether a cached record is one this caller can use.
///
/// The day has to match because spend resets at midnight and yesterday's total
/// must not be shown as today's. The period is checked as well even though the
/// filename now carries it, because a record naming a different period than the
/// file it sits in means something wrote to the wrong place, and a month's total
/// shown as a day's is off by an order of magnitude while still looking like a
/// plausible number.
///
/// A record missing either field predates it, and is rejected rather than
/// guessed at.
fn entry_usable(v: &serde_json::Value, period: Period, day: &str) -> bool {
    v.get("day").and_then(|d| d.as_str()) == Some(day)
        && v.get("period").and_then(|p| p.as_str()) == Some(period.key())
}

/// The record at `path`, if a caller asking for `period` on `day` can use it.
///
/// Takes both rather than reading the clock, so the guard can be exercised
/// against real files without waiting for a day to roll over.
fn read_entry(path: &Path, period: Period, day: &str) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    entry_usable(&v, period, day).then_some(v)
}

fn cache_entry(period: Period) -> Option<serde_json::Value> {
    read_entry(&cache_path(period)?, period, &today())
}

pub fn read_cache(period: Period) -> Option<f64> {
    cache_entry(period)?
        .get("cost")
        .and_then(serde_json::Value::as_f64)
}

/// Writes one record, atomically where the filesystem allows it.
///
/// Takes the path rather than deriving it, so the staging-and-rename dance can
/// be tested against a real directory. It has been wrong before: two writers
/// once shared a single staging name.
fn write_entry(path: &Path, period: Period, day: &str, cost: f64) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let body = format!(
        "{{\"period\":\"{}\",\"day\":\"{day}\",\"cost\":{cost:.4},\"at\":{at}}}",
        period.key()
    );

    // Written beside the target and renamed into place. Two writers can now
    // reach here at once — a live saver's poller and the detached refresher —
    // and a reader that caught a half-written file would fall back to showing no
    // figure at all, which is the exact symptom this file exists to prevent.
    //
    // The staging name carries the pid. A single shared one would have both
    // writers truncating each other mid-write and then renaming the result into
    // place, which is the corruption this is supposed to prevent rather than a
    // cure for it.
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    if std::fs::write(&tmp, &body).is_ok() && std::fs::rename(&tmp, path).is_ok() {
        return Ok(());
    }
    let _ = std::fs::remove_file(&tmp);
    std::fs::write(path, &body)
}

/// `day` is the day the figure is *for*, which the caller takes from the fetch
/// rather than from the clock — see `Fetched::day`.
fn write_cache(period: Period, day: &str, cost: f64) {
    let Some(path) = cache_path(period) else {
        return;
    };
    if let Err(e) = write_entry(&path, period, day, cost) {
        // By this point fetch() has already logged a figure. Without this line
        // the log would show a successful fetch and the screen would keep
        // showing the old number, with nothing to connect the two.
        log(&format!(
            "cache write FAILED  the figure above did not reach {} ({e})",
            path.display()
        ));
    }
}

/// How long ago the usable cached figure was written. `None` means there is
/// nothing usable cached — no run has succeeded, or what is there belongs to a
/// previous day or a different period.
fn cache_age(period: Period) -> Option<Duration> {
    let at = cache_entry(period)?
        .get("at")
        .and_then(serde_json::Value::as_u64)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    // A timestamp in the future means the clock moved backwards — an NTP
    // correction, or a dual-boot machine disagreeing about whether the RTC is
    // UTC. Clamping that to zero would report the cache as just written and
    // suppress every refresh until real time caught up, so it is reported as
    // infinitely old instead: an unreadable age should cause a refresh, not
    // prevent one.
    Some(Duration::from_secs(now.checked_sub(at).unwrap_or(u64::MAX)))
}

// ── Detached cache refresh ───────────────────────────────────────────────────
//
// On a machine in active use the saver is dismissed a second or two after it
// appears, and a fetch has been measured at anything from 1.0s to 195s. So the
// poller below usually never completes a single run: the cache never moves, and
// every later appearance reseeds from the same frozen figure. Every part is
// working and the counter still looks stuck.
//
// The cure is to stop tying the cache to the saver's lifetime. A detached child
// does one fetch and writes the cache whether or not the saver that launched it
// is still on screen, so the next appearance starts from something current even
// if it too lasts two seconds.

/// Skip the refresh when the cache is already this fresh. Without it, a saver
/// appearing every minute would mean a ccusage every minute — and measured
/// against the transcript tree, ccusage is the single most expensive thing this
/// program does.
const CACHE_FRESH_FOR: Duration = Duration::from_secs(60);

fn lock_path() -> Option<PathBuf> {
    app_dir().map(|d| d.join("refresh.lock"))
}

/// Mutual exclusion between refreshers. `None` means one is already in flight
/// and this process should do nothing.
///
/// Deliberately one lock for all periods rather than one each. The point is to
/// stop two ccusage runs from fighting over the same transcript tree, and that
/// contention does not care which window each is asking about. The cost is that
/// switching period while a refresh is running skips one background prefetch,
/// which nothing depends on: the cache files are already separate, and the live
/// saver's poller does not consult this lock at all.
///
/// The lock is the returned open handle, not the file on disk — hold it for as
/// long as the refresh lasts. `share_mode(0)` makes Windows refuse a second
/// opener while it lives, and Windows closes it however this process ends,
/// including a kill and including a panic, which in this binary aborts rather
/// than unwinds and so runs no destructor.
///
/// That is why there is no staleness timeout here. A file left behind by a dead
/// refresher is inert: nothing holds it, so the next opener simply takes it. The
/// alternative — judging abandonment by the file's age — has to guess a
/// threshold above the slowest legitimate fetch, and would still hand the lock
/// to a second refresher while the first was merely slow.
fn take_refresh_lock() -> Option<std::fs::File> {
    let path = lock_path()?;
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .share_mode(0)
        .open(&path)
    {
        Ok(handle) => Some(handle),
        // ERROR_SHARING_VIOLATION: a refresh is already running. The expected
        // outcome, and the only one not worth a log line.
        Err(e) if e.raw_os_error() == Some(32) => None,
        Err(e) => {
            // Anything else disables refreshing entirely, so it has to be
            // visible rather than look like ordinary contention.
            log(&format!("refresh lock unavailable: {e}"));
            None
        }
    }
}

/// Entry point for the detached child: one fetch straight into the cache, no
/// window, no event loop.
pub fn refresh_cache_once() {
    let Some(_lock) = take_refresh_lock() else {
        return;
    };
    // Read here rather than inherited from the launching saver: this is a fresh
    // process, and settings.json is the same answer either way. Only the period
    // matters — the refresher draws nothing, so the scene is none of its
    // business.
    let period = crate::settings::load().period;
    if let Ok(f) = fetch("   [detached]", period) {
        write_cache(period, &f.day, f.cost);
    }
}

/// Launches that child and deliberately does not wait for it.
///
/// Dropping the handle is the intended behaviour, not an oversight: on Windows
/// the child outlives the parent, and waiting for it here would reintroduce
/// exactly the coupling this is meant to break.
pub fn spawn_detached_refresh(period: Period) {
    if cache_age(period).is_some_and(|age| age < CACHE_FRESH_FOR) {
        return;
    }
    // Both failures below are logged, because silence here is indistinguishable
    // from "the cache was already fresh" — and the consequence is that the cache
    // goes back to only advancing when a session outlives a fetch, which is the
    // whole failure this path exists to remove.
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            log(&format!("detached refresh not launched: {e}"));
            return;
        }
    };
    // DETACHED_PROCESS on its own: CREATE_NO_WINDOW is documented as ignored
    // when combined with it, and the release binary is a GUI-subsystem image
    // with no console to suppress in the first place.
    if let Err(e) = Command::new(exe)
        .arg("--refresh-cache")
        .creation_flags(DETACHED_PROCESS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        log(&format!("detached refresh not launched: {e}"));
    }
}

// ── Polling ──────────────────────────────────────────────────────────────────

/// What to show before the first ccusage run finishes. Even the local runner
/// takes about a second, and staring at a blank counter for that long is worse
/// than showing the last figure recorded today.
pub fn initial(period: Period) -> Usage {
    match read_cache(period) {
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

/// How long to wait before the next fetch, given how long the last one took.
///
/// The wait is the remainder of the interval rather than the whole of it, so a
/// fetch costing a second or two does not stretch the cadence by that much on
/// top. That alone is not enough once a fetch outlasts the interval: the
/// remainder comes out zero and node then runs back to back for as long as the
/// saver is up.
///
/// A floor of the last fetch's own duration fixes that, and it is the right
/// floor rather than an arbitrary one. Every fetch re-reads the whole transcript
/// tree — 2071 files and 491 MB on the development machine — and slow fetches
/// are slow precisely because something else is already contending for those
/// files. Polling hardest exactly then makes the contention worse, so the saver
/// was measurably part of its own problem. Holding the wait to the last
/// duration caps the duty cycle at half and changes nothing while fetches are
/// quick.
/// Note that the cadence starts stretching at *half* the interval, not at the
/// interval: a 6s fetch against a 10s interval already waits 6s rather than 4s.
/// Past that point the duty cycle is pinned at one half whatever happens.
fn next_wait(interval: Duration, elapsed: Duration) -> Duration {
    /// Only binds when the interval is under twice this, which at the
    /// configured 10s it never is — one of the other two terms is always at
    /// least 5s. It is here so that lowering REFRESH cannot turn the poller
    /// into a continuous loop.
    const MIN_IDLE: Duration = Duration::from_secs(5);
    interval.saturating_sub(elapsed).max(elapsed).max(MIN_IDLE)
}

// A gate that skipped the poller's fetch when the cache was newer than the
// interval was tried here and removed. It was meant to stop a launch from
// running ccusage twice, once in the detached refresher and once in the poller.
// Measured, it did neither job: the poller's first check usually happens before
// the refresher has written anything, so the duplicate survived, and because the
// poller's own write then made the next iteration skip, the effective refresh
// rate fell from 10s to 20s. The duplicate is documented as a known cost
// instead.

/// Fetches immediately, then on an interval, pushing each result onto the event
/// loop. Stops as soon as the event loop is gone.
pub fn spawn_poller(proxy: EventLoopProxy<UserEvent>, interval: Duration, period: Period) {
    std::thread::spawn(move || {
        loop {
            let started = std::time::Instant::now();

            // Announce the run before making it. A fetch can take several
            // seconds, and on the first launch of a new day there is no cached
            // figure to show meanwhile, so the page needs to know the blank is
            // temporary.
            let mut pending = initial(period);
            pending.freshness = Freshness::Loading;
            if proxy.send_event(UserEvent::Usage(pending)).is_err() {
                return;
            }

            // fetch() writes its own log line, so every call site is covered.
            let usage = match fetch("", period) {
                Ok(f) => {
                    write_cache(period, &f.day, f.cost);
                    Usage {
                        cost: Some(f.cost),
                        freshness: Freshness::Fresh,
                    }
                }
                Err(e) => {
                    eprintln!("[usage] {e}");
                    initial(period)
                }
            };
            if proxy.send_event(UserEvent::Usage(usage)).is_err() {
                return;
            }
            std::thread::sleep(next_wait(interval, started.elapsed()));
        }
    });
}

/// Headless check: run the whole pipeline once and report, without opening a
/// window.
pub fn print_once() {
    let period = crate::settings::load().period;
    let (since, until) = window(period);
    let started = std::time::Instant::now();
    println!("period  = {} ({since}..{until})", period.key());
    println!(
        "runners = {:?}",
        runners().iter().map(|r| r.label).collect::<Vec<_>>()
    );
    match fetch("   [--print-usage]", period) {
        Ok(f) => {
            write_cache(period, &f.day, f.cost);
            println!(
                "total   = ${:.4}   [{:?} via {}]",
                f.cost,
                started.elapsed(),
                f.runner
            );
            println!("cache  = {:?}", cache_path(period));
            println!("reread = {:?}", read_cache(period));
            println!("log    = {:?}", log_path());
        }
        Err(e) => println!("FAILED after {:?}\n{e}", started.elapsed()),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────
//
// Only the two functions that turn outside input into a decision. Everything
// else in this file either shells out or draws on the environment, and is
// covered by `--print-usage` instead.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_with_no_usage_is_zero_not_a_failure() {
        let json = br#"{"daily":[],"totals":{"totalCost":0}}"#;
        assert_eq!(parse_total(json), Ok(0.0));
    }

    #[test]
    fn the_total_comes_from_totals_not_the_daily_rows() {
        let json = br#"{"daily":[{"period":"2026-08-06","totalCost":1.0}],
                        "totals":{"totalCost":137.96}}"#;
        assert_eq!(parse_total(json), Ok(137.96));
    }

    #[test]
    fn a_missing_total_is_an_error_rather_than_a_zero() {
        // Showing $0.00 for a day that had spend is worse than showing the
        // previous figure and marking it stale.
        let json = br#"{"daily":[{"period":"2026-08-06"}],"totals":{}}"#;
        assert!(parse_total(json).is_err());
    }

    #[test]
    fn output_that_is_not_json_is_an_error() {
        assert!(parse_total(b"ccusage: command not found").is_err());
    }

    fn temp_runtime(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clawd-saver-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `node_exe` only checks that the path names a file, so an empty one stands
    /// in for a node install without needing a real interpreter.
    fn stub_node(dir: &Path) -> PathBuf {
        let exe = dir.join("stub-node.exe");
        std::fs::write(&exe, b"").unwrap();
        exe
    }

    #[test]
    fn a_recorded_path_is_preferred() {
        let dir = temp_runtime("recorded");
        let exe = stub_node(&dir);
        std::fs::write(dir.join("node.txt"), exe.to_string_lossy().as_bytes()).unwrap();
        assert_eq!(node_exe(&dir), exe.to_string_lossy());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_bom_and_trailing_newline_do_not_break_the_recorded_path() {
        let dir = temp_runtime("bom");
        let exe = stub_node(&dir);
        let body = format!("\u{feff}{}\r\n", exe.to_string_lossy());
        std::fs::write(dir.join("node.txt"), body.as_bytes()).unwrap();
        assert_eq!(node_exe(&dir), exe.to_string_lossy());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_recording_never_yields_nothing_to_run() {
        // A node moved by a version manager must not drop the bundled ccusage
        // out of the chain: the result is either a real file or the bare name
        // for CreateProcess to resolve against PATH.
        let dir = temp_runtime("stale");
        std::fs::write(dir.join("node.txt"), br"Z:\moved-away\node.exe").unwrap();
        let resolved = node_exe(&dir);
        assert!(resolved == "node" || PathBuf::from(&resolved).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn entry(period: &str, day: &str) -> serde_json::Value {
        serde_json::json!({ "period": period, "day": day, "cost": 1.0, "at": 0 })
    }

    #[test]
    fn a_cache_entry_for_today_and_this_period_is_used() {
        assert!(entry_usable(
            &entry("7d", "20260807"),
            Period::Last7Days,
            "20260807"
        ));
    }

    #[test]
    fn a_cache_entry_from_another_day_or_period_is_refused() {
        let cases = [
            (entry("7d", "20260806"), Period::Last7Days, "yesterday's week"),
            (
                entry("1d", "20260807"),
                Period::Last7Days,
                "today's other period",
            ),
            (
                entry("wtd", "20260807"),
                Period::Last7Days,
                "the calendar week, not the rolling one",
            ),
            (
                entry("30d", "20260806"),
                Period::Last7Days,
                "wrong on both counts",
            ),
        ];
        for (v, period, what) in cases {
            assert!(!entry_usable(&v, period, "20260807"), "{what} was accepted");
        }
    }

    #[test]
    fn a_written_record_reads_back_as_what_was_written() {
        // The predicate above is pinned in isolation; this is the plumbing
        // around it — the format string, the staging file, the rename.
        let dir = temp_runtime("cache-roundtrip");
        let path = dir.join("last-7d.json");
        write_entry(&path, Period::Last7Days, "20260810", 1234.5678).unwrap();

        let v = read_entry(&path, Period::Last7Days, "20260810").expect("not readable");
        assert_eq!(v.get("cost").and_then(serde_json::Value::as_f64), Some(1234.5678));
        assert!(v.get("at").and_then(serde_json::Value::as_u64).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_written_record_is_refused_from_another_day_or_period_on_disk() {
        let dir = temp_runtime("cache-guard");
        let path = dir.join("last-7d.json");
        write_entry(&path, Period::Last7Days, "20260810", 99.0).unwrap();

        assert!(read_entry(&path, Period::Last7Days, "20260809").is_none(), "yesterday");
        assert!(read_entry(&path, Period::Today, "20260810").is_none(), "other period");
        assert!(read_entry(&path, Period::Last7Days, "20260810").is_some(), "its own");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writing_leaves_the_record_and_nothing_else() {
        // The staging file is renamed into place, not copied. One left behind
        // would accumulate per pid, and a reader could pick it up.
        let dir = temp_runtime("cache-staging");
        let path = dir.join("last-30d.json");
        for cost in [1.0, 2.0, 3.0] {
            write_entry(&path, Period::Last30Days, "20260810", cost).unwrap();
        }
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["last-30d.json"], "stray files in the cache directory");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_record_written_where_the_directory_does_not_exist_yet_still_lands() {
        // First run on a machine with no install directory.
        let dir = temp_runtime("cache-fresh");
        let path = dir.join("nested").join("last-1d.json");
        write_entry(&path, Period::Today, "20260810", 7.5).unwrap();
        assert!(read_entry(&path, Period::Today, "20260810").is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cache_entry_predating_the_period_key_is_refused() {
        // What every last.json on disk looked like before the counter could be
        // set to a week or a month. Reading one as though it were the current
        // period would show a day's total for a month, or the reverse.
        let old = serde_json::json!({ "day": "20260807", "cost": 306.89, "at": 0 });
        for period in Period::ALL {
            assert!(!entry_usable(&old, period, "20260807"));
        }
    }

    #[test]
    fn the_epoch_and_its_neighbours_anchor_the_date_conversions() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn every_day_over_a_decade_survives_a_round_trip() {
        // Cheap exhaustive check across leap years, century-adjacent years and
        // every month length, which beats hand-picking cases.
        for z in days_from_civil(2020, 1, 1)..=days_from_civil(2030, 12, 31) {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z, "{y:04}-{m:02}-{d:02}");
        }
    }

    /// 2026-08-07 is a Friday, so `dow` is 5 wherever these tests use that date.
    /// `SUN` and `MON` are the two answers a locale actually gives.
    const FRI: u32 = 5;
    const SUN: u32 = 0;
    const MON: u32 = 1;

    fn since(period: Period, y: i32, m: u32, d: u32, dow: u32, starts: u32) -> String {
        window_ending(period, y, m, d, dow, starts).0
    }

    #[test]
    fn today_is_a_single_date_not_an_empty_range() {
        assert_eq!(
            window_ending(Period::Today, 2026, 8, 7, FRI, SUN),
            ("20260807".into(), "20260807".into())
        );
    }

    #[test]
    fn a_rolling_week_counts_today_as_one_of_its_seven_days() {
        // Not 20260731: an eight-day range would overstate every week by a day.
        assert_eq!(
            window_ending(Period::Last7Days, 2026, 8, 7, FRI, SUN),
            ("20260801".into(), "20260807".into())
        );
    }

    #[test]
    fn rolling_windows_reach_back_across_month_and_year_boundaries() {
        assert_eq!(since(Period::Last30Days, 2026, 8, 7, FRI, SUN), "20260709");
        assert_eq!(since(Period::Last30Days, 2026, 3, 5, 0, SUN), "20260204");
        assert_eq!(since(Period::Last7Days, 2026, 1, 3, 0, SUN), "20251228");
        assert_eq!(since(Period::Last30Days, 2026, 1, 1, 4, SUN), "20251203");
    }

    #[test]
    fn february_is_counted_as_it_actually_falls() {
        // 2024 is a leap year, 2026 is not, and 1900 was not despite being
        // divisible by four — the case a naive rule gets wrong.
        let day_before = |y| since(Period::Last7Days, y, 3, 1, 0, SUN);
        assert_eq!(&day_before(2024)[4..], "0224"); // 2024-02-24, six days back
        assert_eq!(days_from_civil(2024, 3, 1) - days_from_civil(2024, 2, 29), 1);
        assert_eq!(days_from_civil(2026, 3, 1) - days_from_civil(2026, 2, 28), 1);
        assert_eq!(days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28), 1);
        assert_eq!(days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 29), 1);
    }

    /// What GetLocaleInfoEx leaves in the buffer for a one-digit answer.
    fn locale_says(digit: char) -> ([u16; 8], i32) {
        let mut buf = [0u16; 8];
        buf[0] = digit as u16;
        (buf, 2) // the digit and its terminator
    }

    #[test]
    fn each_locale_answer_maps_to_the_right_weekday() {
        // Windows counts from Monday, this file counts from Sunday. Getting the
        // shift backwards would move every week by a day and still look sane.
        for (digit, expected, name) in [
            ('0', 1, "Monday"),
            ('1', 2, "Tuesday"),
            ('2', 3, "Wednesday"),
            ('3', 4, "Thursday"),
            ('4', 5, "Friday"),
            ('5', 6, "Saturday"),
            ('6', 0, "Sunday"),
        ] {
            let (buf, written) = locale_says(digit);
            assert_eq!(parse_first_day(&buf, written), expected, "{name}");
        }
    }

    #[test]
    fn an_unusable_locale_answer_falls_back_to_monday() {
        const MONDAY: u32 = 1;
        let (buf, _) = locale_says('6'); // Sunday, so a fallback is visible
        assert_eq!(parse_first_day(&buf, 0), MONDAY, "call failed");
        assert_eq!(parse_first_day(&buf, 1), MONDAY, "terminator only");
        assert_eq!(parse_first_day(&locale_says('7').0, 2), MONDAY, "out of range");
        assert_eq!(parse_first_day(&locale_says('x').0, 2), MONDAY, "not a digit");
        assert_eq!(parse_first_day(&[0xD800, 0, 0, 0, 0, 0, 0, 0], 2), MONDAY, "lone surrogate");
    }

    #[test]
    fn this_week_starts_where_the_locale_says_it_does() {
        // Friday 2026-08-07: back to Sunday the 2nd, or Monday the 3rd. Getting
        // this from the wrong end of the week is a figure off by up to six days.
        assert_eq!(since(Period::WeekToDate, 2026, 8, 7, FRI, SUN), "20260802");
        assert_eq!(since(Period::WeekToDate, 2026, 8, 7, FRI, MON), "20260803");
    }

    #[test]
    fn this_week_on_its_own_first_day_is_just_that_day() {
        // Sunday 2026-08-02 under a Sunday-start locale, and the Monday after
        // under a Monday-start one. Neither may reach back a whole week.
        assert_eq!(since(Period::WeekToDate, 2026, 8, 2, SUN, SUN), "20260802");
        assert_eq!(since(Period::WeekToDate, 2026, 8, 3, MON, MON), "20260803");
    }

    #[test]
    fn this_week_reaches_back_into_the_previous_month_and_year() {
        // Tuesday 2026-09-01 belongs to a week that started in August.
        assert_eq!(since(Period::WeekToDate, 2026, 9, 1, 2, SUN), "20260830");
        // Thursday 2026-01-01 belongs to a week that started in 2025.
        assert_eq!(since(Period::WeekToDate, 2026, 1, 1, 4, SUN), "20251228");
    }

    #[test]
    fn this_month_starts_on_the_first() {
        assert_eq!(since(Period::MonthToDate, 2026, 8, 7, FRI, SUN), "20260801");
        assert_eq!(since(Period::MonthToDate, 2026, 1, 1, 4, MON), "20260101");
        assert_eq!(since(Period::MonthToDate, 2024, 2, 29, 4, SUN), "20240201");
    }

    #[test]
    fn the_weekday_only_matters_to_this_week() {
        // Every other period must ignore it, so a wrong weekday cannot quietly
        // shift a rolling window or the start of a month.
        for dow in 0..7 {
            for starts in [SUN, MON] {
                for p in [
                    Period::Today,
                    Period::MonthToDate,
                    Period::Last7Days,
                    Period::Last30Days,
                ] {
                    assert_eq!(
                        since(p, 2026, 8, 7, dow, starts),
                        since(p, 2026, 8, 7, FRI, SUN),
                        "{p:?} moved with dow={dow} starts={starts}"
                    );
                }
            }
        }
    }

    /// Weekday of a day number, derived from the epoch rather than from the
    /// code under test: 1970-01-01 was a Thursday, and 0 = Sunday.
    fn weekday(z: i64) -> u32 {
        ((z % 7 + 7 + 4) % 7) as u32
    }

    #[test]
    fn the_epoch_anchor_for_weekdays_is_right() {
        assert_eq!(weekday(days_from_civil(1970, 1, 1)), 4); // Thursday
        assert_eq!(weekday(days_from_civil(2026, 8, 9)), 0); // Sunday
        assert_eq!(weekday(days_from_civil(2026, 8, 7)), 5); // Friday, as FRI
    }

    #[test]
    fn this_week_always_lands_on_the_locales_first_day_and_never_reaches_back_a_whole_week() {
        // Every weekday against every possible locale answer. The check is
        // independent of the formula: the day it picks must *be* the locale's
        // first day, and must be somewhere in the last seven days.
        let mut checked = 0;
        for offset in 0..14 {
            let z = days_from_civil(2026, 8, 1) + offset;
            let (y, m, d) = civil_from_days(z);
            let dow = weekday(z);
            for starts in 0..7 {
                let (sy, sm, sd) = window_start(Period::WeekToDate, y, m, d, dow, starts);
                let sz = days_from_civil(sy, sm, sd);
                assert_eq!(
                    weekday(sz),
                    starts,
                    "{y:04}-{m:02}-{d:02} (dow {dow}) with weeks starting {starts} \
                     began on {sy:04}-{sm:02}-{sd:02}, a different weekday"
                );
                let back = z - sz;
                assert!(
                    (0..=6).contains(&back),
                    "{y:04}-{m:02}-{d:02} reached back {back} days"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 14 * 7);
    }

    #[test]
    fn every_period_produces_a_window_that_ends_today_and_does_not_start_later() {
        for p in Period::ALL {
            let (s, u) = window_ending(p, 2026, 8, 7, FRI, SUN);
            assert_eq!(u, "20260807", "{p:?} does not end today");
            assert!(s <= u, "{p:?} starts after it ends: {s}..{u}");
        }
    }

    const TEN: Duration = Duration::from_secs(10);

    #[test]
    fn a_quick_fetch_keeps_the_requested_cadence() {
        // 1.5s of work then 8.5s of waiting is still a 10s cycle, which is what
        // the interval asks for.
        assert_eq!(
            next_wait(TEN, Duration::from_millis(1500)),
            Duration::from_millis(8500)
        );
    }

    #[test]
    fn a_fetch_slower_than_the_interval_backs_off_instead_of_running_flat_out() {
        // The regression this guards: subtracting a 16s fetch from a 10s
        // interval used to leave a 5s floor, so the saver spent three quarters
        // of a busy period re-reading the transcript tree it was competing for.
        for secs in [11, 16, 60, 195] {
            let elapsed = Duration::from_secs(secs);
            let wait = next_wait(TEN, elapsed);
            assert!(wait >= elapsed, "{secs}s fetch waited only {wait:?}");
        }
    }

    #[test]
    fn the_cadence_stretches_from_half_the_interval_not_from_the_interval() {
        // Documented wrongly once, in two files: a 6s fetch is well under the
        // 10s interval but already pushes the cycle to 12s, because the wait is
        // floored at the fetch's own duration.
        assert_eq!(next_wait(TEN, Duration::from_secs(4)), Duration::from_secs(6));
        assert_eq!(next_wait(TEN, Duration::from_secs(6)), Duration::from_secs(6));
        assert_eq!(next_wait(TEN, Duration::from_secs(8)), Duration::from_secs(8));
    }

    #[test]
    fn there_is_always_a_gap_between_fetches() {
        assert!(next_wait(TEN, Duration::ZERO) >= Duration::from_secs(5));
        assert!(next_wait(Duration::ZERO, Duration::ZERO) >= Duration::from_secs(5));
    }

    #[test]
    fn an_absent_recording_is_not_an_error() {
        let dir = temp_runtime("absent");
        let resolved = node_exe(&dir);
        assert!(!resolved.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
