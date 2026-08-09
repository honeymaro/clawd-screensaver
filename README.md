# Clawd Saver

A Windows screensaver that shows your Claude Code spend while Clawd mines for it.

![Clawd swings a pickaxe into an ore block while today's spend is shown above him](docs/demo.gif)

| Path | What it is |
|---|---|
| `saver/` | The screensaver: a Rust binary that embeds the pages and reads ccusage. |
| `saver/src/ui.html` | The whole visual. Open it in any browser — with no host to feed it, the counter just reads `$--.--`. |
| `saver/src/settings.html` | The settings dialog, same deal: openable on its own, inert without a host. |
| `checks/` | Non-visual verification for both pages. |
| `docs/` | Design and build notes, plus the recording above. |

## Install

```powershell
.\install.ps1                 # build, install, register (5 min idle)
.\install.ps1 -Timeout 10     # 10 minutes instead
.\install.ps1 -Uninstall      # remove it again
```

Everything lands under `%LOCALAPPDATA%\clawd-saver\` and `HKCU:\Control Panel\Desktop`.
No administrator rights required.

The install also drops a pinned copy of ccusage into `runtime\` beside the
binary, with pnpm or npm — whichever is on PATH. It needs node and about 4 MB.
That step is not allowed to fail the install: if it does, the saver falls back to
`pnpx` and says so in the log. Pass `-SkipRuntime` to skip it deliberately.

The Windows screensaver dropdown only lists `.scr` files that live in `System32`,
so this one will not appear there. It still runs on idle — Windows reads the path
from the registry, not from the dropdown. Pass `-System` from an elevated prompt
if you want it in the list.

## Settings

One choice: how far back the counter reaches. **Right-click the `.scr` and pick
Configure**, or from a prompt:

```powershell
Start-Process "$env:LOCALAPPDATA\clawd-saver\clawd-saver.scr" -Verb config
```

Not `& "...clawd-saver.scr" /c`, which looks right and is not: Windows hands
`.scr` files to the shell association rather than running them directly, and
`scrfile`'s `open` verb is `"%1" /S` — so the switch is dropped and the
screensaver starts instead. The `config` verb runs the file with no arguments at
all, which is why a bare invocation has to mean the settings dialog.

| Option | Window | Resets |
|---|---|---|
| Today | today only | daily |
| This week | since the week began | weekly |
| This month | since the 1st | monthly |
| Last 7 days | today and the six before it | never |
| Last 30 days | today and the twenty-nine before it | never |

All five end today. The calendar ones answer *how much so far this period* and
drop to near nothing when it rolls over; the rolling ones answer *what am I
spending lately* and never reset. On the 1st of a month, **This month** reads a
few dollars while **Last 30 days** still reads the real rate.

Which day a week starts on comes from the Windows locale rather than a guess —
Sunday here, Monday across most of Europe. So on its own first day, **This week**
shows exactly what **Today** shows.

The choice lands in `%LOCALAPPDATA%\clawd-saver\settings.json`. Delete it, or
write something it cannot parse, and the counter falls back to today rather than
refusing to start.

The dropdown's own Settings button has the same `System32` problem as the list
above, which is why the command is spelled out here.

## How it works

Rust + [`tao`](https://crates.io/crates/tao) + [`wry`](https://crates.io/crates/wry) —
the window and webview crates that Tauri is built on, used directly without the
Tauri framework, CLI, bundler, or IPC layer. WebView2 ships with Windows 11, so
nothing is bundled. The release binary is **663 KB**, both pages included.

Both pages are embedded with `include_str!`, so the `.scr` is a single
self-contained file.

The saver and the settings dialog share one WebView2 profile, at
`%LOCALAPPDATA%\clawd-saver\webview2`. Left to itself WebView2 derives that
folder from the running module's path, and Windows does not always spell that
path the same way — the long and 8.3-short forms of one binary had each grown
their own profile of the same cached page, 70 MB and 74 MB. `install.ps1` deletes
the old `*.WebView2` folders if it finds them.

That profile is where the install's size actually goes: about 72 MB of it
against 4 MB of ccusage and 663 KB of screensaver, ~77 MB in total. None of it
is written by this program — half is components the WebView2 runtime fetches for
itself and a page of coloured rectangles will never use, Widevine DRM and a
subresource filter list among them. `-Uninstall` takes it with the rest.

### Two things that were not obvious

**WebView2 eats the input.** It covers the whole window and consumes mouse and
keyboard events before tao's event loop sees them. A screensaver that cannot
detect input never closes. The exit trigger therefore lives in an injected script
inside the page and comes back over IPC. It ignores the first 600 ms and requires
8 px of real cursor travel, because Windows sometimes emits a phantom `mousemove`
the instant the saver appears.

**There is no exit on focus loss** — deliberately. It is not part of the
screensaver contract, and it self-destructs on multi-monitor setups: creating the
second window blurs the first, which would quit before anyone saw it.

**A destroyed window does exit, though.** The poller runs off the event loop, not
off any window, so handling only `CloseRequested` once left a saver alive with no
window at all — invisible, running ccusage every ten seconds, and holding the
`.scr` open so it could not be replaced.

### Data

Runners are tried in order and the first one that produces a number wins:

```
<node.exe recorded at install> runtime\node_modules\ccusage\src\cli.js
ccusage                          (a global install, if there is one)
pnpx ccusage@20.0.19
npx -y ccusage@20.0.19
%LOCALAPPDATA%\pnpm\pnpx.CMD     (a PATH that lost the shim but kept node)
```

Each is asked for `daily --json --since <first day> --until <today>`, and the
figure shown is `totals.totalCost` over that range — so the selected period costs
nothing extra: a 30-day window measures the same 0.8–0.9 s as a one-day one,
because ccusage walks the whole transcript tree either way.

The bundled copy is first because it is the only runner that consults neither
PATH nor a package resolver. Measured on the development machine:

| Runner | Normal PATH | PATH with no node or pnpm |
|---|---|---|
| bundled copy | **1.0 s** | **1.0 s** |
| `pnpx` | 2–13 s, ~20 s on the first run of a day | fails in 0.08 s |

Both columns are the reason it exists. `pnpx` spends nearly all of that time
resolving the package rather than running it, and a screensaver the system
launches can inherit a PATH with no package runner on it at all — which is what
"the counter only ever showed `$--.--`" turned out to be.

Those are shell measurements. Inside a running saver both are slower: the log has
recorded the bundled copy at up to 6.8 s and `pnpx` at up to 94 s. Read the table
as a ratio, not as timings to expect.

Everything below the bundled copy is a fallback, for an install that predates the
`runtime\` directory or one where node has since moved. A stale `node.txt` is not
fatal either: the default installer location is tried before the bundled copy is
given up on.

The version is pinned rather than `@latest`, which would re-resolve against the
npm registry on every run and fail offline. `install.ps1` reads it out of
`saver/src/usage.rs`, so the bundled copy and the `pnpx` fallback cannot drift
apart.

The alternatives — a global install, the `ccstats` crate, and reading the
transcripts natively — are measured and compared in
[docs/2026-08-06-usage-data-path.md](docs/2026-08-06-usage-data-path.md). The
period picker and what it touched are in
[docs/2026-08-07-selectable-spend-window.md](docs/2026-08-07-selectable-spend-window.md).

A second is still too long to block the first frame, so the counter is seeded
from `%LOCALAPPDATA%\clawd-saver\last-<period>.json` and updated when the fetch
lands. One file per period, each keyed by date: yesterday's total is never shown
as today's, a month's total is never shown as a day's, and switching back to a
period already fetched today shows its figure immediately rather than starting
from nothing.

**The cache is refreshed by a process that outlives the saver.** Entering `/s`
launches a detached copy of the binary with `--refresh-cache`, which fetches
once, writes the cache and exits. Without it the cache moved only when a session
lasted longer than a fetch — and on a machine in use the saver is dismissed a
second or two after it appears, so the figure could sit frozen for an hour while
nothing was actually broken. Nothing is launched when the cache is under 60 s
old, and a `refresh.lock` keeps two refreshers from overlapping.

Refresh runs every **10 seconds** while the saver is up, and the poller waits
`max(interval - fetch, fetch)`, floored at 5 s. Up to half the interval that is
just the remainder, so the cadence holds. Past half — 5 s here, which the 9.4 s
busy-load median below clears easily — the wait is the fetch's own duration and
the duty cycle is pinned at one half. A 16 s fetch used to be followed by another
5 s later, so the saver spent a busy period re-reading the same transcripts it
was competing for.

A launch whose cache is stale runs ccusage twice, once in the refresher and once
in the poller. That is the price of the cache advancing regardless of how long
the session lasts; the two are usually staggered by WebView2 startup rather than
running together.

Change the cadence in `saver/src/saver.rs`:

```rust
const REFRESH: Duration = Duration::from_secs(10);
```

Fetches are slow for a reason that looks external to this program. ccusage walks
the whole transcript tree — thousands of files and hundreds of megabytes here,
and only ever more of both — so its wall-clock tracks whatever else is touching
those files. Grouped by a proxy for that: median 3.6 s
idle against 9.4 s while Claude Code is working, worst case 62.8 s against
195 s. The day's own spend costs nothing at all, and having the saver open costs
about 1.5× machine-wide. The workings are in
[docs/2026-08-07-fetch-latency-and-cache-freshness.md](docs/2026-08-07-fetch-latency-and-cache-freshness.md).

### When the counter will not fill in

A screensaver has no console, so `%LOCALAPPDATA%\clawd-saver\log.txt` is the only
place to see what happened. One line per session and per fetch — real lines,
gathered onto one day to show every shape in one place:

```
2026-08-09 09:36:48  saver start   1 display(s), 1d, seed=none
2026-08-09 09:36:51  fetch ok         2.5s  via pnpx   1d  $163.78
2026-08-09 09:37:04  fetch FAILED     0.9s  20260809..20260809
    ccusage: exit code: 1 - 'ccusage' is not recognized ... | pnpx: ...
    PATH=C:\Windows\System32
2026-08-09 17:20:31  settings      period=mtd
2026-08-09 17:22:04  saver start   1 display(s), mtd, seed=none
2026-08-09 17:22:04  fetch ok         1.3s  via local  mtd  $1788.83   [detached]
2026-08-09 17:22:05  fetch ok         1.3s  via local  mtd  $1788.83
```

A `[detached]` tag means the refresher, not the poller — that is how a ccusage
running with nothing on screen is accounted for.

`via` is the part to read first. Anything other than `local` means the bundled
copy was missing or unusable and the fetch fell through to a runner that costs
seconds and needs a package runner on PATH — long enough that the saver can be
dismissed before the figure ever lands, which reads as a counter stuck on
`$--.--`. Re-running `install.ps1` puts the bundled copy back.

The log is capped and trims itself.

## Command line

Windows passes `/s`, `/c`, `/c:<hwnd>`, or `/p <hwnd>`. A bare invocation means
config — which is not a fallback but the main route in, since the `config` shell
verb runs the file with no arguments at all.

`/c` opens the settings dialog described above. The `<hwnd>` in `/c:<hwnd>` is
accepted and then ignored: the dialog is a normal top-level window centred on the
primary display rather than one owned by the Control Panel property sheet. That
only shows up if you install with `-System`, since nothing else passes a handle.

`/p` — the thumbnail inside that dialog — is deliberately unimplemented and exits
immediately, leaving the preview black. Both would cost a WebView2 instance; the
difference is that `/c` pays it once because someone asked, while `/p` would
repaint a postage-stamp pane every time the list scrolls.

`--refresh-cache` is not a development switch: it is how a running saver
identifies the detached child it launches to refresh the cache. It opens no
window.

Development switches:

| Flag | Effect |
|---|---|
| `--windowed` | Ordinary 1280×800 window instead of taking over every display |
| `--exit-after <ms>` | Quit on a timer, for unattended checks |
| `--ignore-input` | Do not wire up the input exit, so a screenshot can be taken |
| `--diag` | Have the page report viewport metrics over IPC |
| `--print-usage` | Run the ccusage pipeline once and print. Debug builds only — release is a GUI subsystem binary with no stdout. |

```powershell
cd saver
cargo run -- --windowed
cargo run -- --print-usage
```

Pass these to the **`.exe`**, never to the installed `.scr`. Arguments written on
the command line of a `.scr` are discarded — see Settings above — so
`& "...clawd-saver.scr" --windowed --exit-after 5000` does not open a window with
a timer, it starts a full-screen screensaver with no way out but the mouse.

## Rendering notes

Everything is drawn as axis-aligned rectangles on a 112×74 unit grid, scaled to an
**even integer** number of device pixels per unit. Even, because the arms are 7.5
units wide and half-units still have to land on whole pixels.

All motion is quantised to whole units. Sub-pixel movement would smear the block
edges the character is made of. The pickaxe swing is four discrete poses rather
than a rotation, for the same reason — `ctx.rotate()` would destroy the grid.

Diagonals need care. Stepping a 3-unit block by a full 3 units along a diagonal
makes consecutive blocks meet at their corners only, and the result reads as a
row of loose squares rather than a shaft. `blockLine()` walks the line at
half-block spacing and snaps to the half-unit grid, so the pickaxe stays solid at
every angle. `checks/verify-ui.js` asserts that consecutive blocks actually
overlap, not just that they are in bounds.

On a near-black background there is nothing darker to cast a shadow with, so the
ground under Clawd and the ore is a faintly *lighter* bar instead.

Burn-in protection drifts the whole scene by a few whole units every half minute.

The process calls `SetProcessDpiAwarenessContext` before creating any window.
Without it WebView2 reports `innerWidth` in physical pixels while
`devicePixelRatio` still claims 1.5, and any canvas sizing that multiplies the two
double-counts the scale factor and overflows the screen.

## Verification

```powershell
node checks/verify-ui.js        # every rect, 4 poses x 5 drift offsets, in bounds
node checks/smoke-ui.js         # executes ui.html against a stubbed DOM
node checks/smoke-settings.js   # drives settings.html and asserts what it posts
cd saver; cargo test            # date windows, cache keys, backoff, IPC messages
```

`verify-ui.js` mirrors the layout constants from `ui.html`; if you move the scene,
update both. It refuses to run if a constant it mirrors has changed on one side
only.

The amount row shrinks rather than overflowing. A 30-day total is the string that
can get long — this machine sits around $6,400 for thirty days, so roughly 60%
more spending would reach five figures, and `$12345.67` at the normal scale would
push the stale marker off the stage once burn-in drift is applied. `verify-ui.js`
checks the widths that make the step happen and the ones that must not.

All non-visual on purpose, so checks do not require taking over the display.
