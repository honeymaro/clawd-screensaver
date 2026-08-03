# Clawd Saver — Design and Build Notes

Written 2026-08-03.

A real Windows screensaver (`.scr`) that shows today's Claude Code spend while Clawd mines for it.

---

## 1. Stack

### Decision: Rust + `tao` + `wry`, without the Tauri framework

`wry` is the webview crate Tauri is built on and `tao` is its window crate. Both
can be used independently of Tauri. This takes the webview core and drops
Tauri's CLI, config files, IPC layer, bundler, and plugin system.

Measured environment (2026-08-03):

```
rustc 1.91.0   host x86_64-pc-windows-msvc
link.exe       VS 2022 BuildTools 14.42.34433      present
WebView2       150.0.4078.105 (ships with Win11)   present
pnpm 10.28.0 / pnpx                                present
node v24.16.0
go / dotnet / gcc / zig                            absent
```

**No new toolchain to install.** That was the single biggest reason for this choice.

### Alternatives rejected

| Option | Size | Why not |
|---|---|---|
| Electron | ~200 MB | Ruled out by the user |
| Tauri v2 | ~5 MB | Drags a CLI, bundler, and IPC layer along for something wry alone does |
| Go + go-webview2 | ~3 MB | Go toolchain not installed |
| C# + WebView2 | ~15 MB | .NET SDK not installed |
| C++ Win32 + WebView2 | ~300 KB | Smallest, but hundreds of lines of COM boilerplate |
| HTA (mshta.exe) | 0 bytes | Trident/IE11 renderer, so no ES6; a `.scr` must be a PE; trips EDR |
| Raw `windows-rs` + `webview2-com` | ~800 KB | 300–400 extra lines of unsafe to save ~2 MB — bad trade |

If size ever becomes a problem, the last row is the escape hatch.

### Release profile

```toml
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

Target was under 3 MB. See §9 for what it actually came out at.

---

## 2. Data path

### Measured

> **Correction, re-measured during implementation.** The 1.9 s in the first draft
> of this plan was an underestimate taken on an idle machine early in the day.
> The real figure is **4 s idle, up to 9 s under load**. It does not change the
> conclusion — it strengthens it.

```
pnpx ccusage@20.0.19 --version                 4.63s   <- pnpx resolution + node startup
pnpx ccusage@latest  --version                 4.86s
pnpx  pinned  daily --json (today filter)      8.21s   <- + ~3.5s of ccusage scanning
pnpx  latest  daily --json (today filter)      8.10s
cmd /C pnpx pinned daily --json                6.83s   <- cmd wrapping costs nothing
```

More than half the latency is **pnpx package resolution and node startup**, not
ccusage doing work. Neither `--offline` nor pinning a version reduces it (4.63 vs
4.86 is noise). Only a globally installed `ccusage` skips that 4.6 s outright,
which is why it sits first in the runner chain.

### Data contract (confirmed against real output)

```json
{
  "daily": [ { "period": "2026-08-03", "totalCost": 2.3299,
               "modelsUsed": ["claude-opus-5"], "totalTokens": 1126579 } ],
  "totals": { "totalCost": 2.3299, "inputTokens": 31, "outputTokens": 46133,
              "cacheCreationTokens": 66973, "cacheReadTokens": 1013442,
              "totalTokens": 1126579 }
}
```

- The value used is `totals.totalCost`, in USD.
- A day with no usage returns an empty `daily` array; treat that as zero, not an error.
- `ccusage statusline` is **unsuitable** — it is built for Claude Code hooks, wants
  JSON on stdin, and emits no JSON of its own.

### Constraint: the first paint must not wait on ccusage

Waiting several seconds on a subprocess would start the screensaver as a black
screen. So:

1. Window and webview come up **immediately** and the animation starts.
2. The amount is seeded from the cache at `%LOCALAPPDATA%\clawd-saver\last.json`.
   With no cache it reads `$--.--`.
3. A worker thread runs ccusage, then hands the result to the main thread through
   `EventLoopProxy`, which injects it with `evaluate_script`.
4. On success the cache is rewritten.

### Runner fallback chain

```
cmd /C ccusage daily --json --since <today> --until <today>     (global install, if any)
  fails -> cmd /C pnpx ccusage@<pinned> daily --json ...
  fails -> cmd /C npx -y ccusage@<pinned> daily --json ...
  fails -> %LOCALAPPDATA%\pnpm\pnpx.CMD ...
  fails -> keep the cached value and mark it stale on screen
```

- `pnpx` and `npx` are `.CMD` batch files, so they go through
  `Command::new("cmd").args(["/C", ...])`; executing them directly runs into
  Windows argument-escaping problems.
- `creation_flags(0x08000000)` (`CREATE_NO_WINDOW`) stops a console window from
  flashing over the screensaver.
- The absolute-path entry exists because a screensaver may be launched by the
  system with a thinner PATH than an interactive shell.

### Refresh interval

Originally 5 minutes, on the reasoning that the user is idle while the saver is up
so the figure barely moves. Changed to **10 seconds** on request — see §9.

---

## 3. Screensaver contract

### Arguments

Parsed case-insensitively, accepting either `/` or `-`.

| Argument | Meaning | Implementation |
|---|---|---|
| `/s` | Run full screen | The real thing |
| `/c`, `/c:<hwnd>` | Settings dialog | A single MessageBox; there are no settings |
| `/p <hwnd>` | Preview thumbnail in the settings pane | **Exits immediately**, leaving it black |
| none | Bare invocation | Treated as `/c` |

`/p` would mean standing up a WebView2 instance inside a postage-stamp pane owned
by someone else's HWND. Not worth it.

### Exit conditions — the trap

**WebView2 covers the whole window and consumes mouse and keyboard input before
tao's event loop ever sees it.** A screensaver that cannot detect input is a
screensaver that will not close, so the trigger has to live inside the page.

- An injected script listens for `mousemove`, `keydown`, `mousedown`, and `wheel`,
  then calls `window.ipc.postMessage("exit")`.
- `with_ipc_handler` on the Rust side receives it and shuts everything down.
- The mouse only counts after **8 px of travel from the first observed position**,
  because Windows sometimes emits a phantom `mousemove` the moment the saver appears.

### Multiple displays

`event_loop.available_monitors()` enumerates the displays and each gets a window.

- Primary: the full scene.
- Secondary: the same page with `CLAWD_STATIC` set, which paints one frame and stops.
- Input on any window closes all of them.

Animating every display would mean one renderer per monitor running flat out.
Reusing the same page in a static mode is the cheapest way to avoid that without a
second code path.

### Window attributes

`decorations(false)`, `always_on_top(true)`, `fullscreen(Borderless(monitor))`,
`set_cursor_visible(false)`.

---

## 4. The screen

`saver/src/ui.html` is embedded with `include_str!`, so the `.scr` is one
self-contained file with no external assets.

1. **Amount, not a counter.** A 3×5 bitmap font with `$`, `.`, `-`, and `?` glyphs
   added alongside the digits. Wider amounts grow outward from the centre.

2. **States**
   - loading with nothing cached: `$--.--` with a light running along the dashes
   - loading over a figure already on screen: unchanged, no dimming
   - stale: the cached figure, dimmed, with a small marker beside it
   - failed with nothing cached: `$--.--`, static

3. **Spend increase.** If a poll returns more than the last one, the ore shatters
   immediately instead of waiting for the swing cycle. While the user is idle this
   almost never fires, which is correct.

4. **Burn-in protection.** The whole scene drifts by a few whole units every half
   minute. Whole units only, so the pixel grid never breaks.

5. **Static mode.** No `requestAnimationFrame` loop; a single frame.

---

## 5. Layout

```
claude-screensaver/
├─ README.md
├─ install.ps1                   place the .scr and register it
├─ checks/
│  ├─ verify-ui.js               rect alignment and bounds, all poses and drifts
│  └─ smoke-ui.js                executes the page against a stubbed DOM
├─ docs/
│  └─ 2026-08-03-clawd-saver-plan.md
└─ saver/
   ├─ Cargo.toml
   └─ src/
      ├─ main.rs                 entry point, argument dispatch, DPI awareness
      ├─ saver.rs                windows, monitors, IPC exit, usage injection
      ├─ usage.rs                runner chain, JSON parsing, cache I/O, polling
      └─ ui.html                 the embedded screen
```

Three dependencies: `tao`, `wry`, `serde_json`. The `windows` crate is deliberately
absent — `CREATE_NO_WINDOW` is a raw flag through
`std::os::windows::process::CommandExt`, and the two user32 calls that are needed
(`MessageBoxW`, `SetProcessDpiAwarenessContext`) are declared inline.

`#![windows_subsystem = "windows"]` keeps a console from appearing in release builds.

---

## 6. Phases

Each phase was independently verifiable.

### Phase 0 — skeleton
`cargo new`, add `tao` and `wry`, one borderless full-screen window on the primary
monitor, the page embedded and loaded with `with_html()`.
**Done when** the mining animation runs full screen.

### Phase 1 — screensaver contract
Argument dispatch, IPC-based exit with the 8 px threshold, hidden cursor,
always-on-top, monitor enumeration, static mode for secondary displays.
**Done when** a small mouse movement closes every window at once and `/p` exits
immediately.

### Phase 2 — data
`usage.rs`: the runner chain with `CREATE_NO_WINDOW`, `totals.totalCost` parsing,
date-keyed cache, worker thread through `EventLoopProxy` into `evaluate_script`,
periodic polling.
**Done when** the window appears instantly and the figure updates a few seconds
later, and a broken PATH leaves the cached value on screen.

### Phase 3 — the screen
Amount rendering with the extra glyphs, the three states, the spend-increase
shatter, burn-in drift, the dark palette.
**Done when** every state reproduces on screen.

### Phase 4 — packaging
Release profile, measured binary size, `.exe` copied to `.scr`, `install.ps1`
writing `SCRNSAVE.EXE` / `ScreenSaveActive` / `ScreenSaveTimeOut` under
`HKCU:\Control Panel\Desktop`.
**Done when** Windows starts it on idle by itself.

---

## 7. Open questions at planning time

| Question | Resolution |
|---|---|
| Pin the ccusage version or track `@latest`? | **Pinned** (`ccusage@20.0.19`). No speed gain, but it works offline. |
| Idle timeout default | **5 minutes**, changeable with `install.ps1 -Timeout`. |
| Dark mode | **Switched to a screensaver-only dark palette.** Background `#14120F`. A full-screen cream background is punishing at night. |
| Show anything besides the amount? | No. Amount only. |

---

## 8. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| WebView2 swallows input, saver never closes | Fatal | The IPC approach in Phase 1. **This is why Phase 1 came before the data work.** |
| `pnpx` not on PATH in the screensaver process | No figure shown | Absolute-path fallback in the runner chain |
| Unsigned `.scr` tripping antivirus | Blocked | Personal machine; add an exclusion |
| Not listed in the Windows settings dropdown | Inconvenience | Only System32 is enumerated, but registry registration works regardless |
| `/p` unimplemented | Black preview thumbnail | Accepted trade-off |
| One webview per monitor | Memory | Static mode on secondary displays |

---

## 9. Outcome

All phases complete.

### Measured

| | Planned | Actual |
|---|---|---|
| Release binary | under 3 MB | **623,616 B (0.59 MB)** |
| PE subsystem | GUI | `2` — GUI, no console flash |
| ccusage refresh | 2 s | **4 s idle, up to 9 s under load** (see §2) |
| Input to exit | — | keystroke **176 ms**; mouse movement immediate |
| `/p` exit | immediate | exit 0 in 1.2 s, all of it process startup |
| Arguments Windows actually passes | assumed `/s` | **confirmed `/s`**, logged from a system-triggered launch |

That last row was verified by broadcasting `SC_SCREENSAVE` and recording the real
invocation, rather than trusting documentation. A screen capture attempted at the
same moment failed with `The handle is invalid`, which confirms the saver was
running on the dedicated screensaver desktop.

### Decisions that departed from the plan

1. **No exit on focus loss.** It was in the first implementation and was removed.
   It is not part of the screensaver contract, and it self-destructs on
   multi-monitor setups: creating the second window blurs the first, which would
   quit before anyone saw it.

2. **`SetProcessDpiAwarenessContext` turned out to be mandatory.** Not in the plan.
   Without it WebView2 reports `innerWidth` in physical pixels while
   `devicePixelRatio` still claims 1.5, so any canvas sizing that multiplies the
   two double-counts the scale factor and overflows the screen. Found by logging
   the real viewport numbers out of the page after two wrong guesses.

3. **The stage grew from 88×54 to 112×74 units.** The amount needed a row of its
   own, the dollar sign's stem needed headroom above it, and a wider aspect ratio
   keeps a full-screen display from being mostly side margin.

4. **The diamond icon was dropped.** A 3×3 plus shape read as a medical cross, not
   a gem. The `$` glyph does the same job, so the icon was redundant.

5. **The dollar sign is 3×7, not 3×5.** Squeezed into the digit body it was just a
   blocky S. The stem has to overhang above and below to read as currency, so
   `drawText` vertically centres over-tall glyphs against the five-row baseline.

6. **The pickaxe is drawn as an overlapping block line.** Stepping a 3-unit block
   by a full 3 units along a diagonal makes consecutive blocks meet at their
   corners only, and it reads as a row of loose squares. `blockLine()` walks at
   half-block spacing snapped to the half-unit grid, which stays solid at every
   angle. `verify-ui.js` now asserts consecutive blocks overlap, not merely that
   they are in bounds — the bounds check alone missed this.

7. **No halo behind the head lamp, and the shadow is lighter than the background.**
   A translucent rectangle overhanging the helmet dome read as a misaligned box.
   And on a near-black background there is nothing darker to cast a shadow with,
   so the ground is a faintly lighter bar instead.

8. **The refresh interval is 10 seconds, not 5 minutes.** Requested after seeing it
   run. The poller sleeps only the remainder of the interval, because a flat sleep
   after a 4–9 s fetch would stretch the cadence to 14–19 s. This means node runs
   roughly 40–80% of the time the saver is up; a global ccusage install roughly
   halves that.

9. **Verification is entirely non-visual.** Full-screen test launches were
   disrupting the user's actual work, so checks moved to `checks/verify-ui.js`
   (rect alignment and bounds across every pose and drift offset) and
   `checks/smoke-ui.js` (eight usage states executed against a stubbed DOM).

### Found in use, 2026-08-04

The first launch on a new day showed `$--.--` and looked broken. It was not: the
cache is keyed by date, so a new day starts with no figure, and the fetch takes
3.5–5 seconds to fill it in. The defect was that `$--.--` was rendered
identically whether a fetch was in flight or had failed, with nothing moving to
say which — so every morning read as a failure for several seconds.

The poller now announces a run before making it (`Freshness::Loading`) and the
page runs a light along the dashes while it waits. A refresh over a figure that
is already on screen leaves it alone rather than dimming it every ten seconds.
`checks/smoke-ui.js` asserts the highlight actually travels, since a stationary
one would be the original bug again.

Worth noting for future debugging: two earlier attempts to reproduce this
concluded the process was crashing. Both were wrong — one was the liveness poll
sampling before the process had spawned, the other was the screensaver being
dismissed by ordinary input. Neither was the app.

### Not verified

- **Multiple displays.** This machine has one monitor, so the enumeration and
  static-mode code paths have never been executed.
- **`prefers-reduced-motion`** shares a branch with static mode and is covered by
  the smoke test, but was never exercised with the OS setting actually enabled.
