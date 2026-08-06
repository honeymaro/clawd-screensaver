# Clawd Saver

A Windows screensaver that shows today's Claude Code spend while Clawd mines for it.

![Clawd swings a pickaxe into an ore block while today's spend is shown above him](docs/demo.gif)

| Path | What it is |
|---|---|
| `saver/` | The screensaver: a Rust binary that embeds the page and reads ccusage. |
| `saver/src/ui.html` | The whole visual. Open it in any browser — with no host to feed it, the counter just reads `$--.--`. |
| `checks/` | Non-visual verification for `ui.html`. |
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

## How it works

Rust + [`tao`](https://crates.io/crates/tao) + [`wry`](https://crates.io/crates/wry) —
the window and webview crates that Tauri is built on, used directly without the
Tauri framework, CLI, bundler, or IPC layer. WebView2 ships with Windows 11, so
nothing is bundled. The release binary is **622 KB**.

The page is embedded with `include_str!`, so the `.scr` is a single self-contained
file.

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

### Data

Runners are tried in order and the first one that produces a number wins:

```
<node.exe recorded at install> runtime\node_modules\ccusage\src\cli.js
ccusage                          (a global install, if there is one)
pnpx ccusage@20.0.19
npx -y ccusage@20.0.19
%LOCALAPPDATA%\pnpm\pnpx.CMD     (in case the saver inherits a thin PATH)
```

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
[docs/2026-08-06-usage-data-path.md](docs/2026-08-06-usage-data-path.md).

A second is still too long to block the first frame, so the counter is seeded
from `%LOCALAPPDATA%\clawd-saver\last.json` and updated when the fetch lands. The
cache is keyed by date — yesterday's total is never shown as today's.

Refresh runs every **10 seconds**, and the poller sleeps only the remainder of
the interval rather than the whole of it, with a 5 s floor. Without the floor a
fetch that outlasts the interval leaves node running continuously for as long as
the saver is up.

Change the cadence in `saver/src/saver.rs`:

```rust
const REFRESH: Duration = Duration::from_secs(10);
```

### When the counter will not fill in

A screensaver has no console, so `%LOCALAPPDATA%\clawd-saver\log.txt` is the only
place to see what happened. One line per session and per fetch:

```
2026-08-06 09:36:48  saver start   1 display(s), seed=none
2026-08-06 09:36:51  fetch ok         2.5s  via pnpx  $163.78
2026-08-06 09:37:04  fetch FAILED     0.9s
    ccusage: exit code: 1 - 'ccusage' is not recognized ... | pnpx: ...
    PATH=C:\Windows\System32
2026-08-06 13:28:09  saver start   1 display(s), seed=$202.19 cached
2026-08-06 13:28:10  fetch ok         1.0s  via local  $202.19
```

`via` is the part to read first. Anything other than `local` means the bundled
copy was missing or unusable and the fetch fell through to a runner that costs
seconds and needs a package runner on PATH — long enough that the saver can be
dismissed before the figure ever lands, which reads as a counter stuck on
`$--.--`. Re-running `install.ps1` puts the bundled copy back.

The log is capped and trims itself.

## Command line

Windows passes `/s`, `/c`, `/c:<hwnd>`, or `/p <hwnd>`. A bare invocation means
config.

`/p` — the settings-dialog thumbnail — is deliberately unimplemented and exits
immediately, leaving the preview black. Standing up a WebView2 instance in a
postage-stamp pane costs far more than it is worth.

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

## Rendering notes

Everything is drawn as axis-aligned rectangles on a 112×70 unit grid, scaled to an
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
node checks/verify-ui.js      # every rect, 4 poses x 5 drift offsets, in bounds
node checks/smoke-ui.js       # executes the page against a stubbed DOM
```

`verify-ui.js` mirrors the layout constants from `ui.html`; if you move the scene,
update both.

Both are non-visual on purpose, so checks do not require taking over the display.
