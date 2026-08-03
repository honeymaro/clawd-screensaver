//! Reads today's Claude Code spend by shelling out to ccusage.

use std::{
    os::windows::process::CommandExt,
    path::PathBuf,
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tao::event_loop::EventLoopProxy;

use crate::saver::UserEvent;

/// Pinned instead of `@latest`. `@latest` re-resolves against the npm registry
/// on every single run, which costs a round trip and fails outright offline.
const CCUSAGE: &str = "ccusage@20.0.19";

/// Stops a console window from flashing onto the screensaver when we shell out.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    /// Straight from a successful ccusage run.
    Fresh,
    /// ccusage failed but a previous value was on disk.
    Stale,
    /// ccusage failed and there is nothing cached.
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub struct Usage {
    pub cost: Option<f64>,
    pub freshness: Freshness,
}

impl Usage {
    pub fn to_json(self) -> String {
        let state = match self.freshness {
            Freshness::Fresh => "ok",
            Freshness::Stale => "stale",
            Freshness::Failed => "error",
        };
        match self.cost {
            Some(c) => format!("{{\"cost\":{c:.4},\"state\":\"{state}\"}}"),
            None => format!("{{\"cost\":null,\"state\":\"{state}\"}}"),
        }
    }
}

/// Local calendar date as `YYYYMMDD`. ccusage groups by local timezone, so a UTC
/// date would report the wrong day for part of every evening.
fn today() -> String {
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
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(st: *mut SystemTimeFields);
    }
    let mut st = SystemTimeFields::default();
    unsafe { GetLocalTime(&mut st) };
    format!("{:04}{:02}{:02}", st.year, st.month, st.day)
}

/// Runners are tried in order. `pnpx` and `npx` are `.CMD` batch files, so they
/// have to go through `cmd /C` rather than being executed directly.
///
/// Measured on this machine: `pnpx` spends ~4.6s on package resolution and node
/// startup before ccusage does any work, and ccusage's own scan adds ~3.5s on
/// top. A globally installed `ccusage` skips the first half entirely, so it is
/// tried first; when it is absent `cmd` fails within milliseconds and the chain
/// moves on.
fn runners() -> Vec<(String, Vec<String>)> {
    let mut list: Vec<(String, Vec<String>)> = vec![
        ("ccusage".into(), vec![]),
        ("pnpx".into(), vec![CCUSAGE.into()]),
        ("npx".into(), vec!["-y".into(), CCUSAGE.into()]),
    ];
    // A screensaver can be launched by the system with a thinner PATH than an
    // interactive shell, so fall back to where pnpm installs itself.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let direct = PathBuf::from(local).join("pnpm").join("pnpx.CMD");
        if direct.exists() {
            list.push((direct.to_string_lossy().into_owned(), vec![CCUSAGE.into()]));
        }
    }
    list
}

pub fn fetch() -> Result<f64, String> {
    let day = today();
    let mut problems = Vec::new();

    for (program, lead) in runners() {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(&program).args(&lead).args([
            "daily", "--json", "--since", &day, "--until", &day,
        ]);
        cmd.creation_flags(CREATE_NO_WINDOW);

        match cmd.output() {
            Ok(out) if out.status.success() => match parse_total(&out.stdout) {
                Ok(cost) => return Ok(cost),
                Err(e) => problems.push(format!("{program}: {e}")),
            },
            Ok(out) => problems.push(format!(
                "{program}: exit {} — {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(e) => problems.push(format!("{program}: {e}")),
        }
    }
    Err(problems.join(" | "))
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

fn cache_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|d| PathBuf::from(d).join("clawd-saver").join("last.json"))
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

/// What to show before the first ccusage run finishes. A ccusage run takes
/// roughly two seconds, and staring at a blank counter for that long is worse
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
/// The wait is the remainder of the interval, not the whole of it. A fetch takes
/// 7-9s, so sleeping the full interval on top would stretch a requested 10s
/// cadence into 18s.
pub fn spawn_poller(proxy: EventLoopProxy<UserEvent>, interval: Duration) {
    std::thread::spawn(move || {
        loop {
            let started = std::time::Instant::now();
            let usage = match fetch() {
                Ok(cost) => {
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
            std::thread::sleep(interval.saturating_sub(started.elapsed()));
        }
    });
}

/// Headless check: run the whole pipeline once and report, without opening a
/// window.
pub fn print_once() {
    let started = std::time::Instant::now();
    match fetch() {
        Ok(cost) => {
            write_cache(cost);
            println!("today ({}) = ${cost:.4}   [{:?}]", today(), started.elapsed());
            println!("cache  = {:?}", cache_path());
            println!("reread = {:?}", read_cache());
        }
        Err(e) => println!("FAILED after {:?}\n{e}", started.elapsed()),
    }
}
