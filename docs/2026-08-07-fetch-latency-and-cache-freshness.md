# Clawd Saver — Why the Counter Froze, and What Slows a Fetch

Written 2026-08-07.

The counter sat on the same figure for nearly an hour while every part of the
data path was working. This is what the measurements found and what changed as a
result.

Extends [the usage data path note](2026-08-06-usage-data-path.md), whose §2 said
the in-session slowdown had no established cause. It has one now, and it is not
the screensaver.

---

## 1. The symptom

`$306.89` on screen from 16:02 to 16:58 on 2026-08-06, while ccusage run by hand
answered `$316.14`. No errors: the log held 868 successful fetches and not one
failure. (That count is every line the file held at the time, not a count for
that day alone, and the log has since rotated past it — the excerpts below are a
snapshot rather than something reproducible from `log.txt` today.)

The log explained it in one glance. Of the last eight sessions, six produced a
`saver start` line and no fetch line at all:

```
16:21:10  saver start   1 display(s), seed=$306.89 cached
16:25:05  saver start   1 display(s), seed=$306.89 cached
16:28:30  saver start   1 display(s), seed=$306.89 cached
16:44:42  saver start   1 display(s), seed=$306.89 cached
16:57:54  saver start   1 display(s), seed=$306.89 cached
16:57:59  fetch ok         5.1s  via local  $306.89     <- the one that survived
17:01:45  saver start   1 display(s), seed=$306.89 cached
17:21:09  saver start   1 display(s), seed=$306.89 cached
```

A fetch line is written when the fetch *returns*. No line means the process died
first. With a one-minute idle timeout on a machine in use, the saver appears
constantly and is dismissed a second or two later — reliably less than a fetch
takes.

**And it is self-sustaining.** The cache was only ever written by a successful
fetch, so a session too short to finish one left the cache untouched, and the
next session seeded from the same frozen figure. Nothing recovers this except a
session long enough to complete a fetch.

---

## 2. What actually makes a fetch slow

Fetch duration is bimodal: a 1.0 s floor with excursions past 100 s. Four
measurements, in the order that ruled things out.

**Not the day's volume.** Three runs each against an almost-empty day and a
fully loaded one, interleaved:

```
20260807 (near-empty)   1.07  1.07  1.02  s
20260806 (heavy)        0.99  1.00  1.02  s
node -e 0               0.16  0.12        s
```

Identical. ccusage walks the whole transcript tree either way — 2071 files and
491 MB — and the day filter changes what it reports, not what it reads.

**Not the screensaver.** Measured inside a running saver with WebView2
animating, against a shell at the same moment:

| | Fetch |
|---|---|
| Inside the saver, animating | 2.5 / 1.9 / 1.8 / 1.5 s |
| Shell, concurrent with the above | 1.51 / 1.59 / 1.44 s |
| Shell, no saver running | 0.90 / 1.04 / 1.07 s |

Having the saver up costs about 1.5× machine-wide. Whether being *inside* it
costs anything beyond that is not settled: the inside samples average 1.93 s
against the concurrent shell's 1.51 s, so if anything the data leans the other
way, and three or four samples cannot separate that from noise. What the
comparison does rule out is an effect large enough to explain a 100× tail.

**Not CPU.** Saturating all 12 cores moved the fetch from 1.0 s to 1.35 s. A
1.4× effect cannot produce a 100× tail.

**It is contention for the transcripts themselves.** Grouping logged fetches by
the Claude Code spend booked around the same time — a proxy for how hard
something else was writing to the same tree — 811 of 827 fell into a bucket with
enough neighbours to estimate a rate:

| Concurrent spend | n | p50 | p90 | p99 | max |
|---|---|---|---|---|---|
| idle, < $0.5/min | 758 | 3.6 s | 10.9 s | 25.8 s | 62.8 s |
| busy, ≥ $0.5/min | 53 | 9.4 s | 34.0 s | 195.0 s | 195.0 s |

2.6× at the median, 3.1× at p90, and 7.6× at p99. The busy bucket is 7% of the
samples and holds everything slower than 62.8 s.

**Two buckets, not three, and that is the second version of this table.** The
first grouped each fetch by the spend booked since the previous fetch, which
looked reasonable and was not: spend is only recorded when a turn completes, so
a fetch slow enough to span a quiet stretch shows a delta of zero and gets filed
as idle. That metric put the 195 s outlier in the idle bucket and produced a
third `> $3/min` tier of three samples with a 16.4 s median. Re-derived from
spend booked per ten-minute clock bucket, the third tier disappears entirely
(n=0) and the 195 s sample moves to busy, where the surrounding log shows $34 of
spend in five minutes. An earlier draft of this note attributed that outlier to a
release build; it was heavy Claude Code use, and the attribution was a guess.

Counts grow as the log does, and it rotates. These are from 2026-08-07.

**This lands at the worst possible moment.** A screensaver appears just after
someone stops working — which is exactly when the transcripts were last written
and the machine is still busy. The saver systematically samples the slow end of
the distribution.

---

## 3. The saver was part of its own problem

`REFRESH` is 10 s and the poller slept the remainder of the interval with a 5 s
floor. When a fetch took 16 s the remainder was zero, so the floor applied and
the next fetch started 5 s later: a 76% duty cycle re-reading 491 MB, during the
one period when something else was already contending for those files.

The wait is now `max(interval - elapsed, elapsed)`, still with the 5 s floor. The
floor is the last fetch's own duration rather than an arbitrary constant because
that duration *is* the congestion signal.

The crossover is at **half** the interval, not at the interval — a 6 s fetch
against a 10 s interval already waits 6 s and stretches the cycle to 12 s. Since
the busy-bucket median above is 9.4 s, the saver is in that regime whenever it
matters. The 5 s floor is consequently dead at the configured 10 s interval, one
of the other two terms always being at least 5 s; it survives only to stop a
smaller `REFRESH` from producing a continuous loop.

---

## 4. Decision: the cache no longer depends on the saver's lifetime

A saver entering `/s` launches a detached copy of itself with `--refresh-cache`,
which does one fetch, writes the cache, and exits. It opens no window and does
not join the event loop.

`DETACHED_PROCESS` is the point: the child survives the saver being dismissed, so
a two-second appearance still leaves the next one something current to seed from.
It is started before the window is built, so the fetch is already in flight while
WebView2 initialises.

Verified by polling the process table while the parent exited on its own timer
mid-fetch:

```
parent exited at        : 2.24s
orphan child observed   : True
all clawd procs gone at : 2.98s
cache                   : $3.00 (planted)  ->  $250.96
```

**Two guards, because a refresher is the most expensive thing this program
does.**

*A freshness gate.* Nothing is spawned when the cached figure is under 60 s old.
Without it, a saver appearing every minute would mean a ccusage every minute all
day — which is precisely the contention §2 blames for the slow tail. Verified: a
launch against a seconds-old cache spawned no child at all.

*A lock.* `refresh.lock` is opened with `share_mode(0)`, and the lock is the open
handle rather than the file. A second refresher gets a sharing violation and
exits; Windows closes the handle however the holder ends, including a kill and
including a panic, which in this binary aborts rather than unwinds and so runs no
destructor. A file left behind by a dead refresher is inert — nothing holds it,
so the next opener takes it.

The first version judged abandonment by the file's age instead, reclaiming
anything older than 300 s. That needs a threshold above the slowest legitimate
fetch (195 s observed, and no timeout bounds it), would hand the lock to a second
refresher while the first was merely slow, and left the process-kill case relying
on a destructor that `panic = "abort"` skips. The handle has none of those
questions and is shorter.

*A per-writer staging file.* Cache writes go to a `.tmp.<pid>` beside the target
and are renamed into place. The first version used one shared `last.tmp`, which
with two writers meant they could truncate each other mid-write and then rename
the result into place — manufacturing the torn read the rename was added to
prevent.

(The cache was a single `last.json` at the time. It is now one file per period,
for reasons in [the period note](2026-08-07-selectable-spend-window.md).)

**A launch with a stale cache runs ccusage twice**, once in the refresher and
once in the poller's first iteration:

```
15:38:19  saver start   1 display(s), seed=$19.00 cached
15:38:20  fetch ok         1.8s  via local  $335.99   [detached]
15:38:21  fetch ok         1.8s  via local  $335.99
```

This is a known cost, not an oversight, and an attempt to remove it was reverted.
Skipping the poller's fetch when the cache is newer than the interval sounds
right and measures wrong: the poller's first check almost always lands before the
refresher has written anything, so the duplicate survived — and because the
poller's own write then made the *next* iteration skip, the effective refresh
rate halved from 10 s to 20 s. Making it reliable means the poller waiting on the
child, which reintroduces the coupling §4 exists to remove.

Two walks of the contended tree per launch is the price of the cache advancing
whether or not the session outlives a fetch. They are usually staggered rather
than simultaneous, because WebView2 startup delays the poller.

Log lines say who asked, so a ccusage running with nothing on screen is
explicable:

```
14:02:42  fetch ok    2.3s  via local  $250.96   [detached]
14:03:04  saver start   1 display(s), seed=$251.09 cached
```

---

## 5. What this does not fix

The first figure shown is still up to 60 s stale, by design. Closing that gap
means refreshing more often, and §2 is the argument against it.

A long session on a busy machine still shows a figure that lags by tens of
seconds. Nothing here makes ccusage faster; the fix would have to be an
incremental reader that does not re-walk 491 MB per call, which is the native
option costed in [§3 of the previous note](2026-08-06-usage-data-path.md).

The measurements were all taken on one machine with one display, and the spend
rate used to bucket §2 is a proxy for disk activity rather than a measurement of
it. The direction is solid; the coefficients are not portable.
