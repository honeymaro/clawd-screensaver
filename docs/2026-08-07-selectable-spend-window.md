# Clawd Saver — A Selectable Spend Window

Written 2026-08-07.

The counter can be set to today, this week, this month, the last 7 days or the
last 30 days, chosen in a settings dialog. The five decisions worth recording,
and what the feature turned up elsewhere while it was being built.

---

## 1. Both readings of "a week", because they answer different questions

"A week" and "a month" each have two meanings, and the first version of this
offered only one of them: today, and rolling windows of 7 and 30 days. The
argument was
that a calendar month resets to almost nothing overnight on the 1st, so the
number a screensaver exists to surface disappears for the first few days of every
month.

That is true and it is not a reason to withhold the option. *This month* is what
a bill looks like; *Last 30 days* is what a spending rate looks like. Someone
watching a budget wants the first precisely **because** it resets. Both are
offered:

| | Starts |
|---|---|
| Today | today |
| This week | the most recent first-day-of-week |
| This month | the 1st |
| Last 7 days | six days back |
| Last 30 days | twenty-nine days back |

Every window includes today, which is why a seven-day window reaches back six
days and not seven. Off by one the other way and every week silently reports
eight days.

### The week has to start somewhere, and it is not this program's call

The earlier version dodged this by not offering a calendar week at all, on the
grounds that Sunday-versus-Monday is a locale question. It still is — so the
answer comes from the locale: `GetLocaleInfoEx(LOCALE_IFIRSTDAYOFWEEK)`, which
reports Sunday here and Monday across most of Europe. Getting it wrong is not
cosmetic; it moves the window by up to six days.

Windows numbers that answer 0 = Monday while `GetLocalTime` numbers weekdays
0 = Sunday, so the two are shifted into the same scale before use. A locale that
declines to answer falls back to Monday, the ISO week.

One consequence worth knowing rather than debugging: on its own first day, *This
week* is identical to *Today*. That is correct, and it is what the counter showed
the first time it was checked — on a Sunday, under a Sunday-start locale.

## 2. Date arithmetic without a date crate

`--since`/`--until` need a calendar date `n` days back, which means real civil
date arithmetic: month lengths, leap years, the century rule.

Howard Hinnant's `days_from_civil` / `civil_from_days` are written out in
`usage.rs` instead. Thirty lines against a dependency, in a file that already
declares `GetLocalTime` through a hand-written `extern` block for exactly the
same reason — this project picked wry over Tauri on size and rejected `ccstats`
partly for pulling in SQLite.

They are covered by a test that round-trips **every day from 2020 to 2030**,
which is cheaper to write than choosing interesting cases and catches more:
every month length, every leap year, both century behaviours.

## 3. The dialog is a WebView2 window, not a Win32 dialog

`Mode::Config` already existed and showed a message box saying there were no
settings. It now opens a real window built on the tao and wry already in the
binary.

A radio group and two buttons is a `DialogBoxIndirectParam` template or a pile of
`CreateWindowEx` plus `WM_COMMAND` plumbing, in a project whose entire visual
surface is already a page. The cost is a WebView2 instance, about a second — the
same cost that makes `/p`, the thumbnail repainted every time the settings list
scrolls, still not worth implementing. Here it is paid once, after someone asked
for it.

The dialog is centred on the primary monitor and focused explicitly. A window
revealed with `set_visible` after creation does not necessarily come up focused,
and an unfocused dialog ignores Escape, Enter and the arrow keys the page binds.

**Opening it is not `"clawd-saver.scr" /c`.** Windows hands `.scr` files to the
shell association instead of running them directly, and `scrfile`'s verbs are:

```
config   "%1"
install  rundll32.exe desk.cpl,InstallScreenSaver %l
open     "%1" /S
```

So a switch written on the command line is discarded and `open` starts the
screensaver — which is what happened the first several times this dialog was
tested, and why the arguments appeared to be ignored. `install` opens the Control
Panel page rather than this program. That leaves `config`, which passes no
arguments at all — which is why a bare invocation has to mean the settings
dialog. `Start-Process <path> -Verb config` is that same verb from a prompt.

Measured rather than assumed, because the obvious counter-argument is that
PowerShell's `&` runs a named file through `CreateProcess`, which knows nothing
about verbs. It does not here: `.scr` is not in `PATHEXT`, so `&` falls through
to the association like everything else. Launching `& <path> /c` and matching the
child by parent pid — necessary, because Windows was independently starting the
screensaver during the first attempt at this and confounded it — gives a command
line of `"<path>" /S`. `Start-Process <path>` with no verb gives the same, so
`open` is the default: **a double-click runs the screensaver, it does not open
settings.**

**Verified by screenshot rather than by clicking.** Synthetic input could not be
made to work here: `SetForegroundWindow` from a background test process is
refused by Windows, so no click or keystroke ever reached the window. What is
tested instead is both halves of the contract in isolation — `smoke-settings.js`
drives the page and asserts the message it posts, and `config.rs` unit-tests the
mapping from that message back to a period — plus a `PrintWindow` capture showing
the dialog rendering correctly with the stored choice preselected. The only link
neither covers is WebView2's IPC delivery, which is the same bridge the saver
already relies on to close itself on input.

## 4. One cache file per period

`last.json` becomes one file per period — `last-1d.json`, `last-wtd.json`,
`last-mtd.json`, `last-7d.json`, `last-30d.json` — each
still keyed by day inside. A month's total displayed as a day's would be wrong by
an order of magnitude while still looking like a plausible number, which is the
kind of wrong that does not get noticed.

The first attempt kept the single file and added a `period` field to it, refusing
a record whose period did not match. That protects readers but not writers, and
the writers overlap: the refresher a saver launches keeps running after that
saver is dismissed. Switch the setting, reopen, and the abandoned refresher can
land *after* the new period's figure has been cached and overwrite it — at which
point the next poll finds a record for the wrong period, treats it as nothing,
and blanks a perfectly good number to `$--.--`. Separate files remove the
collision instead of detecting it.

It also turns switching periods from a cost into a non-event: a period already
fetched today still has its figure, so going back to it shows a number
immediately rather than starting from the loading state.

The `period` field stays inside each file even though the name now carries it. A
record naming a period other than the file it sits in means something wrote to
the wrong place, and that is worth refusing rather than trusting. Files written
before the field existed have no period at all and are refused on the same rule.

## 5. A fetch now carries the day it asked about

Threading the period through turned up a bug that predates it. `fetch` worked out
its window when it started; `write_cache` called the clock again when it
finished. A fetch has been measured at over three minutes, and a screensaver is
typically the thing running at midnight, so a fetch spanning the rollover is
close to a nightly event — and its result was then filed under the day it
*finished*. Yesterday's total, stamped today, which is precisely what the cache's
day key exists to prevent.

The poller would correct that within ten seconds. The detached refresher fetches
once and exits, so nothing corrected it there until the next launch.

`fetch` now returns the day alongside the figure and `write_cache` takes it as an
argument, so the two cannot disagree. The guard on the reading side is a pure
predicate with tests: wrong day refused, wrong period refused, and a record
written before the period key existed refused rather than guessed at.

---

## What it cost elsewhere

**The amount row had to learn to shrink.** A 30-day total is the longest string
the counter can produce. This machine is at roughly $6,400 over thirty days —
four figures, comfortably inside what already rendered — but about 60% more
spending crosses into five, and nothing about the row degraded gracefully when
it did. `$12345.67` is 99 units at the normal scale, and once it is
centred with the stale marker beside it and three units of burn-in drift applied,
the right edge lands at 113 on a 112-unit stage. The row now picks the largest
scale that fits inside 96 units and holds its centre line, so a day and a week
render exactly as before and only five figures and up step down.

This was found by arithmetic, not by seeing it break: `verify-ui.js` had never
been given a string longer than `$1234.56`. It now checks up to `$123456.78`, and
separately asserts that the step happens at five figures and not before.

**A saver whose window went away kept running.** Found by hitting it: a
screensaver process from half an hour earlier was still alive with no window at
all, shelling out to ccusage every ten seconds, and holding the handle that made
reinstalling the `.scr` fail. Its log showed a normal `saver start   1
display(s)` and no exit line. The event loop only treated `CloseRequested` as a
reason to stop, and the poller runs off the event loop rather than off any
window, so a window destroyed by anything else left an invisible process doing
the single most expensive thing this program does, forever.

`WindowEvent::Destroyed` now exits too, and logs which surface went. The trigger
in the observed case was a second saver instance starting — which itself only
happened because the binary was replaced underneath a running one, so this is not
a scenario a user would reach. The consequence, a process with no way to ever
stop, is worth guarding against regardless of how it starts.

The settings dialog handles it as well, though it has no poller to run away with.
An invisible process holding the `.scr` open is enough on its own: that is what
made an install fail here, and a four-line arm is cheaper than the comment
explaining why the asymmetry was fine.

**One WebView2 profile, where there had quietly been two.** Adding a second page
meant a second webview, which is what prompted looking at where the first one
kept its cache. wry derives that directory from the running module's path, and
Windows does not spell that path the same way every time: `CLAWD-~1.SCR.WebView2`
and `clawd-saver.scr.WebView2` were sitting side by side, the 8.3-short and long
spellings of one binary. Two profiles of the same page — 70 MB and 74 MB, both
still growing, in a project that rejected a dependency over 5.4 MB. Which launch
produced which was never pinned down and does not change the fix. Both webviews
now name the directory outright, so every entry point converges on one, and
`install.ps1` reclaims the leftovers.

**Nothing measurable in fetch time.** A 30-day window and a one-day window both
take 0.8–0.9 s from a warm shell, because ccusage walks the whole transcript tree
either way and the range only changes what it reports. The
[latency note](2026-08-07-fetch-latency-and-cache-freshness.md) has the rest.
