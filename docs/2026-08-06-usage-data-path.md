# Clawd Saver — The Usage Data Path

Written 2026-08-06.

How the screensaver gets today's spend, and why it gets it that way.

Supersedes the runner chain in §2 of [the 2026-08-03 plan](2026-08-03-clawd-saver-plan.md),
which put a global `ccusage` install first. Everything else in that document still holds.

---

## 1. The problem

The counter showed `$--.--` for the whole time the saver was up, most reliably on
the first launch of a day. Two independent causes, both in the same layer:

**`pnpx` is slow, and it is slow for reasons unrelated to ccusage.** Most of the
wall-clock is package resolution, not work. The first run of a day is worse
because the dlx cache has to be rebuilt. A saver dismissed after a few seconds is
dismissed before the figure ever lands.

**`pnpx` is not always reachable.** A screensaver is launched by the system, not
from a shell, and can inherit a PATH with no node, no pnpm and no npm on it. Every
runner in the original chain resolved through PATH, so all of them failed together.

```
PATH=C:\Windows\System32;C:\Windows
pnpx ccusage@20.0.19 daily --json
  -> 'pnpx' is not recognized as an internal or external command    (0.08s)
```

The second cause is the important one. The first only makes the counter late; the
second makes it impossible.

---

## 2. Decision: install a pinned copy of ccusage beside the binary

`install.ps1` puts ccusage under `%LOCALAPPDATA%\clawd-saver\runtime\` and writes
the absolute path of `node.exe` next to it in `node.txt`. The saver runs that copy
directly. It is the only runner that consults neither PATH nor a package resolver.

Measured on the development machine, same day's data, interleaved:

```
bundled copy   0.94  0.96  1.11  1.17  1.17  1.25  1.29  s
pnpx           2.07  2.55  2.87                          s   (idle machine)
pnpx           2 - 13 s under load, ~20 s on the first run of a day
```

Under a PATH stripped to `System32`:

```
bundled copy   0.97 s, unchanged
pnpx           fails in 0.08 s
```

Cost: 4.1 MB and 19 files in the install directory, and about a second to install
against a warm pnpm store. The `.scr` grows by 2,560 bytes, to 636,416.

### Both are slower inside a running saver

Everything above was measured from a shell. The saver's own log records what
happens while the page is animating a canvas on every display:

```
11:29:01  fetch ok   93.9s  via pnpx
11:29:31  fetch ok   25.4s  via pnpx
13:42:32  fetch ok    4.3s  via local
13:55:20  fetch ok    4.0s  via local
```

Across twelve logged `local` fetches: 1.0 s minimum, 1.8 s median, 6.8 s maximum.

A day later the cause was narrowed down, and it does not look like the saver:
fetch duration tracks whatever else is reading and writing the transcript tree,
and having the saver open at all costs about 1.5× machine-wide rather than that
being a cost specific to fetching from inside it. Whether being inside costs
anything further is unresolved on three or four samples.
[The 2026-08-07 note](2026-08-07-fetch-latency-and-cache-freshness.md) has the
measurements.

The consequence for this document stands either way. The shell figures above are
a best case for both runners rather than a typical one, and what survives
contact with a real session is the ratio between them. Treat every number here
as a comparison, not a promise.

### Why not a global install

A global `ccusage` also skips package resolution, so its wall-clock is comparable.
It is rejected on the other two axes: it still resolves through PATH, so it fails
in exactly the case that matters, and it mutates an environment the screensaver
does not own — the user's global package set, shared with every other tool that
might want a different ccusage.

---

## 3. Alternatives measured and rejected

| Option | Fetch | Agrees with ccusage | Survives a thin PATH | `.scr` size | Footprint |
|---|---|---|---|---|---|
| `pnpx ccusage` | 2–13 s, ~20 s cold | exact | no | +0 | none |
| Global `ccusage` | ~1 s | exact | no | +0 | user's global env |
| **Bundled copy** | **1.0 s** | **exact** | **yes** | **+0** | 4.1 MB, ours |
| [`ccstats`](https://lib.rs/crates/ccstats) 0.4.0 | 17 s | exact | yes | +5.4 MB | none |
| Native reimplementation | 0.7 s | ~1.5% low | yes | small | none |

### `ccstats` — correct, but slower than the thing it would replace

A Rust crate that reimplements ccusage, exposed as both a library and a CLI. The
library surface is two lines:

```rust
let s = ccstats::summarize_cost(ccstats::SummaryOptions::default())?;
s.cost   // Some(158.5550)
```

It is accurate to the cent — `$158.5550` against ccusage's `$158.55502645` on the
same data — and it prices current models correctly, because it fetches the
LiteLLM pricing table at runtime and caches it rather than hardcoding one.

It is rejected on wall-clock. Three consecutive in-process calls took 18.9 s,
17.0 s and 17.4 s; with `offline: true` the same calls took 16.8 s and 17.6 s, so
the cost is not the pricing fetch. It supports several agent sources, indexes into SQLite and walks
the full history, where this project needs one day of one source. `rusqlite` is
bundled: a release binary that does nothing but call `summarize_cost` is
6,061,056 bytes, against 636,416 for the entire screensaver. That is a tenfold
increase for a stack chosen over Tauri on size.

Worth keeping in view as a reference implementation: its LiteLLM fetch, cache and
resolve path is what a native option would need, and it is MIT licensed.

### Native reimplementation — fast, but takes on a maintenance liability

Reading the JSONL transcripts directly runs in 665 ms. It is rejected because of
what it costs to keep correct.

**There is no cost field in the transcripts.** They carry token counts and a model
name, so any native reader has to own a pricing table. Claude pricing moves: the
Sonnet 5 introductory rate expires 2026-08-31, which is a silently wrong counter
on 2026-09-01 rather than a visible failure. Fetching LiteLLM at runtime removes
the staleness but adds a network dependency to a path that has to work offline.

**Two traps make it harder than it looks.** Filtering by file modification time
and summing every message in the matching files overstates a day by more than an
order of magnitude — measured at `$3,618` against a true `$134`. Each message has
to be filtered by its own `timestamp` against the UTC window of the local day, and
deduplicated on `requestId` + `message.id`, or retries are counted twice.

Getting both right still landed at `$135.92` against ccusage's `$137.96` on the
same day, about 1.5% low. Closing that gap means tracking ccusage's cache-tier
accounting, which is the maintenance burden the whole option was meant to avoid.

At 1.0 s the bundled copy is close enough to 0.7 s that trading exactness for it
is a bad deal.

---

## 4. The design as built

```
<node.exe from node.txt> runtime\node_modules\ccusage\src\cli.js
  fails -> cmd /C ccusage daily --json ...              (a global install, if any)
  fails -> cmd /C pnpx ccusage@<pinned> daily --json ...
  fails -> cmd /C npx -y ccusage@<pinned> daily --json ...
  fails -> %LOCALAPPDATA%\pnpm\pnpx.CMD ...
  fails -> keep the cached value and mark it stale on screen
```

**The bundled runner is launched directly, the rest through `cmd`.** `pnpx` and
`npx` are `.CMD` batch files and `Command::new("pnpx")` fails with "program not
found", because CreateProcess appends `.exe` and does not search PATHEXT. Routing
through `cmd` also skips the batch-argument escaping the standard library would
otherwise apply, which is safe only because every argument is a fixed string, an
installer-recorded path or a digits-only date. The bundled runner is a real `.exe`
at an absolute path and needs none of that.

**The version is pinned, and pinned in one place.** `@latest` re-resolves against
the npm registry on every run and fails offline. `install.ps1` reads the version
out of the `CCUSAGE` constant in `saver/src/usage.rs`, so the bundled copy and the
`pnpx` fallback cannot drift apart — a mismatch would quietly mean two different
ccusages depending on which runner won.

**`node.txt` is written without a BOM.** A BOM is not whitespace, so it survives
trimming and would be glued to the front of the path. The reader strips one anyway.

**Nothing in the chain is required.** Verified behaviour:

| Condition | Winner | Fetch |
|---|---|---|
| Everything present | `local` | 0.96 s |
| PATH without node, pnpm or npm | `local` | 0.97 s |
| `node.txt` absent | `local`, via `%ProgramFiles%\nodejs\node.exe` | 1.23 s |
| `runtime\` absent (an install predating it) | `pnpx` | 2.52 s |

The last row is why the old chain is kept rather than replaced.

**The runtime step cannot fail the install.** If node is missing, or the package
manager errors, `install.ps1` warns and carries on. The saver still works through
the fallbacks, and the log says which runner it used.

---

## 5. What would change the decision

- **ccusage stops being distributed on npm**, or the pinned version stops working
  against a newer transcript format. The native option becomes the only one, and
  §3 is the record of what it costs.
- **`ccstats` narrows its scan** to one source and one day, and drops the bundled
  SQLite. It is already exact and already solves pricing staleness; only speed and
  size disqualify it.
- **The 4.1 MB becomes a problem.** It will not — WebView2 is already on the
  machine and dwarfs it — but if the install has to be a single file, the native
  option is the only one that fits.
