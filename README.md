# Clawd Saver

A Windows screensaver that shows your Claude Code spend while Clawd earns it, in
whichever of nine scenes you pick: at the ore face, at a furnace, minding a
rack, reading the bill as it prints, stamping parcels, aiming a dish at
something, cutting fruit out of the air, working as the dog in a game of Duck
Hunt, or fishing off a jetty at night.

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

Two choices: how far back the counter reaches, and what Clawd is doing while it
counts. **Right-click the `.scr` and pick Configure**, or from a prompt:

```powershell
Start-Process "$env:LOCALAPPDATA\clawd-saver\clawd-saver.scr" -Verb config
```

Not `& "...clawd-saver.scr" /c`, which looks right and is not: Windows hands
`.scr` files to the shell association rather than running them directly, and
`scrfile`'s `open` verb is `"%1" /S` — so the switch is dropped and the
screensaver starts instead. The `config` verb runs the file with no arguments at
all, which is why a bare invocation has to mean the settings dialog.

### How much

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

### What Clawd is doing

| Option | What you see | When the figure goes up |
|---|---|---|
| Mining | a pickaxe swung into an ore block | the ore shatters and respawns |
| The forge | coal shovelled into a furnace | the fire flares |
| Server rack | a tablet held beside a rack of blinking drives | a wave of green runs up the bays |
| Night fishing | a jetty, a moon, and a line in the water | something takes the bait |
| The receipt | a printer, and the bill fan-folded into a pile beside it | the lamps go solid and paper flies |
| Parcel line | parcels down a conveyor, stamped one by one | the ink turns bright |
| The uplink | a dish pulsing from its throat out to the rim | the whole dish lights at once |
| The dojo | Clawd in the middle, a sword in each hand, fruit in from both edges | one more sails right across |
| Duck Hunt | a duck up out of the grass, a sight closing on it, and Clawd is the dog | he comes back with two |
| Surprise me | one of the nine, rolled again at every start | — |

Each scene tints the counter with its own colour for that moment — gem for the
mine, flame for the forge, a terminal green for the rack, moonlight for the
jetty, a status red for the printer, stamp violet for the belt, signal blue for
the dish, melon pink for the dojo and a grass green for Duck Hunt — so a rise
reads differently depending on what is on screen.

**The receipt** is the only one whose picture carries the number as well: the
height of the paper pile is the figure on a log scale, so a day's spend leaves a
few sheets and a heavy month stacks up past Clawd's shoulder.

**Surprise me** is rolled once per launch and shared, not rolled per display: a
multi-monitor setup shows the same scene on every screen rather than nine
different ones.

Fifteen options do not fit the dialog, so the form scrolls, and the stored scene
is scrolled into view when it opens. It is not made taller to suit, because a
window sized for the list would hang off the bottom of a 768-line laptop screen
and the window cannot be resized.

Both choices land in `%LOCALAPPDATA%\clawd-saver\settings.json`. Delete it, or
write something it cannot parse, and it falls back to today's spend and the mine
rather than refusing to start. Each field falls back on its own, so a file
written before the scenes existed still selects its period.

The dropdown's own Settings button has the same `System32` problem as the list
above, which is why the command is spelled out here.

## How it works

Rust + [`tao`](https://crates.io/crates/tao) + [`wry`](https://crates.io/crates/wry) —
the window and webview crates that Tauri is built on, used directly without the
Tauri framework, CLI, bundler, or IPC layer. WebView2 ships with Windows 11, so
nothing is bundled. The release binary is **about 700 KB**, both pages included.

Both pages are embedded with `include_str!`, so the `.scr` is a single
self-contained file.

The saver and the settings dialog share one WebView2 profile, at
`%LOCALAPPDATA%\clawd-saver\webview2`. Left to itself WebView2 derives that
folder from the running module's path, and Windows does not always spell that
path the same way — the long and 8.3-short forms of one binary had each grown
their own profile of the same cached page, 70 MB and 74 MB. `install.ps1` deletes
the old `*.WebView2` folders if it finds them.

That profile is where the install's size actually goes: about 69 MB of it
against 4 MB of ccusage and 700 KB of screensaver, ~74 MB in total. None of it
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
place to see what happened. One line per session and per fetch — real lines, with
one failure from an earlier day pulled in so every shape is in one place:

```
2026-08-09 09:37:04  fetch FAILED     0.9s  20260809..20260809
    ccusage: exit code: 1 - 'ccusage' is not recognized ... | pnpx: ...
    PATH=C:\Windows\System32
2026-08-11 19:34:48  fetch ok         1.9s  via local   1d  $550.72   [detached]
2026-08-11 19:34:48  saver start   1 display(s), 1d, mine, seed=$541.59 cached
2026-08-11 19:34:50  fetch ok         1.8s  via local   1d  $552.39
2026-08-11 19:35:05  settings      period=1d scene=random
2026-08-11 19:35:11  saver start   1 display(s), 1d, mine, seed=$552.39 cached
2026-08-11 19:35:17  saver start   1 display(s), 1d, rack, seed=$552.49 cached
2026-08-11 19:35:28  saver start   1 display(s), 1d, forge, seed=$552.59 cached
```

The scene sits between the period and the seed, and it is the resolved one. Those
last three lines are what **Surprise me** looks like from outside: one `settings`
line saying `scene=random`, and then a different scene named at every launch.
Nothing else records what was on screen.

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
| `--scene <name>` | Draw `mine`, `forge`, `rack`, `dock`, `printer`, `belt`, `uplink`, `dojo`, `duckhunt` or `random` for this run only, without touching what is stored. Case-insensitive. An unrecognised name leaves the stored scene alone and says so on stderr, rather than quietly drawing one nobody asked for. |
| `--exit-after <ms>` | Quit on a timer, for unattended checks |
| `--ignore-input` | Do not wire up the input exit, so a screenshot can be taken |
| `--diag` | Have the page report viewport metrics over IPC |
| `--print-usage` | Run the ccusage pipeline once and print. Debug builds only — release is a GUI subsystem binary with no stdout. |

```powershell
cd saver
cargo run -- --windowed
cargo run -- --windowed --scene dock
cargo run -- --print-usage
```

Pass these to the **`.exe`**, never to the installed `.scr`. Arguments written on
the command line of a `.scr` are discarded — see Settings above — so
`& "...clawd-saver.scr" --windowed --exit-after 5000` does not open a window with
a timer, it starts a full-screen screensaver with no way out but the mouse.

Dismiss the real screensaver first, too. Launched while an installed copy was
already up, the dev build created its window, never got a WebView2 process, and
sat there — past `--exit-after`, which is armed after the surfaces are built and
so cannot rescue a hang inside one. Both share a WebView2 profile, which is the
obvious suspect and has not been pinned down; it has been seen once, and eight
launches with nothing else running were all up inside two seconds. Windows will
not start a second screensaver on its own, so this is a development-only trap.

## Rendering notes

Everything is drawn as axis-aligned rectangles on a 112×74 unit grid, scaled to an
**even integer** number of device pixels per unit. Even, because the arms are 7.5
units wide and half-units still have to land on whole pixels.

All motion is quantised to whole units. Sub-pixel movement would smear the block
edges the character is made of. The pickaxe swing is four discrete poses rather
than a rotation, for the same reason — `ctx.rotate()` would destroy the grid.

Clawd himself, the counter, the palette, the particles and the drift are shared;
a scene is mostly what sits beside him, plus what he is wearing. Headgear is
costume rather than anatomy, so it lives in a `HAT` table and each scene names
one: the hard hat and its lamp belong to the mine, not to Clawd. A scene may
also stand him somewhere else and drive his far arm, which the dojo is the only
one to do. Each one is a small object in `ui.html`'s
`SCENES` registry with an `accent` colour, a `celebrate(now)` for the moment the
figure rises, a `draw(now, blinking)` that draws and advances nothing, and — only
if it has state worth advancing — a `step(now, dt)` that advances and draws
nothing. Five of the nine have no `step` at all.

That split is not tidiness. A secondary display paints one frame and then
repaints only when the figure changes, and `prefers-reduced-motion` is honoured
by never stepping, so anything a scene caches in `step` is frozen on both paths.
Poses are therefore derived from `now` inside `draw` — the flame flicker, the
drive LEDs, the swing — and a scene says "gone until this moment" rather than
"gone", so it recovers without being stepped.

`draw` may also read the figure on screen, which is how the receipt knows how
much paper to lay on the floor. That is consistent rather than an exception: the
host sets the figure, not `step`, and a display that repaints only when it
changes repaints exactly when it changes.

The same property is what makes `checks/verify-scenes.js` possible: a frame
rendered at any instant is a real frame. The rest of the reasoning is in
[docs/2026-08-11-four-scenes.md](docs/2026-08-11-four-scenes.md), what the three
added after it needed is in
[docs/2026-08-12-three-more-scenes.md](docs/2026-08-12-three-more-scenes.md), and
the eighth is in [docs/2026-08-12-the-dojo.md](docs/2026-08-12-the-dojo.md), and
the ninth is in [docs/2026-08-13-duck-hunt.md](docs/2026-08-13-duck-hunt.md).

Diagonals need care. Stepping a 3-unit block by a full 3 units along a diagonal
makes consecutive blocks meet at their corners only, and the result reads as a
row of loose squares rather than a shaft. `blockLine()` walks the line at
half-block spacing and snaps to the half-unit grid, so the pickaxe stays solid at
every angle. `checks/verify-ui.js` asserts that consecutive blocks actually
overlap, not just that they are in bounds.

On a near-black background there is nothing darker to cast a shadow with, so the
ground under Clawd and the ore is a faintly *lighter* bar instead. The dojo is
the exception: it has a floor, so the ground is not the background there and it
can afford a shadow that is actually darker. Duck Hunt has grass rather than a
floor and draws no shadow at all, because Clawd is standing in it.

Burn-in protection drifts the whole scene by a few whole units every half minute.

The process calls `SetProcessDpiAwarenessContext` before creating any window.
Without it WebView2 reports `innerWidth` in physical pixels while
`devicePixelRatio` still claims 1.5, and any canvas sizing that multiplies the two
double-counts the scale factor and overflows the screen.

## Verification

```powershell
node checks/verify-ui.js        # every rect, 4 poses x 5 drift offsets, in bounds
node checks/verify-scenes.js    # the same, for every scene in the registry
node checks/smoke-ui.js         # executes ui.html against a stubbed DOM
node checks/smoke-settings.js   # drives settings.html and asserts what it posts
cd saver; cargo test            # date windows, cache keys, backoff, IPC messages
```

`verify-ui.js` mirrors the layout constants from `ui.html`; if you move the scene,
update both. It refuses to run if a constant it mirrors has changed on one side
only.

`verify-scenes.js` takes the other approach: it reads the registry out of
`ui.html` and drives the real page, so a scene added later is checked without
anyone remembering to add it here. Every rect has to land on whole device pixels
and stay inside the stage at all five drift offsets, every frame has to open with
the background clear, every particle has to stay inside the envelope `stepBits`
culls against, and no two registry entries may draw the same thing.

Two things it deliberately allows. A **backdrop** may overhang the stage, but
only if it still covers the whole width and reaches the bottom while doing it —
which is what a sea has to do, and what a misplaced prop cannot fake. And
particles are identified by being exactly 1.5 units square, so scenery that
happens to be that size inherits their looser bounds; that hole is known and not
worth closing with a tag in the page.

What it cannot see at all is a registry key that stops matching `Scene::key()`,
because it takes its scene list from that same registry. `cargo test` covers that
side: `saver.rs` scrapes the registry out of the embedded page and asserts one
entry per scene the settings can select.

The amount row shrinks rather than overflowing. A 30-day total is the string that
can get long — this machine sits around $6,400 for thirty days, so roughly 60%
more spending would reach five figures, and `$12345.67` at the normal scale would
push the stale marker off the stage once burn-in drift is applied. `verify-ui.js`
checks the widths that make the step happen and the ones that must not.

All non-visual on purpose, so checks do not require taking over the display.
