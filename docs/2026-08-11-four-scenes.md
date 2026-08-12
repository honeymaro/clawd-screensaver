# Clawd Saver — Four Scenes

Written 2026-08-11.

Clawd can now be mining, feeding a furnace, minding a rack of drives, or fishing
off a jetty at night, chosen in the settings dialog or left to a per-launch roll.
What had to change to make one scene into four, and the two things that turned
out to be load-bearing.

---

## 1. The refactor came first, and had to change nothing

`ui.html` was one scene written inline: the ore block's colours, its 6×6 map, the
swing poses and the shatter were interleaved with the stage, the palette, the
amount row, Clawd's parts and the particle system. Adding a second scene by
branching inside all of that would have produced a file where no one could tell
which lines belonged to which.

So the split happened on its own, before any new scene existed: everything that
would be shared moved above a divider, the mine moved into a `makeMine()`, and
nothing else changed. The point of doing it as a separate step is that it is
*checkable* — the page must render exactly what it rendered before, and "exactly"
can be taken literally. Driving the page against a stub that records every
`fillRect` gives a deterministic stream: 83,340 rectangles over a fixed run,
identical byte for byte on both sides of the refactor. That was a throwaway
harness rather than one of the checks in `checks/`, since it is only meaningful
against a specific "before".

That is worth more than it sounds. A pixel-art scene that shifts by one unit does
not announce itself, and a screenshot comparison would have missed it.

## 2. The contract is three rules, and two of them are about time

A scene is an object with three members — an `accent` colour, a `celebrate(now)`
and a `draw(now, blinking)` — plus a `step(now, dt)` only if it has state worth
advancing. Two of the four do not. Three rules:

- **`step` advances and draws nothing.**
- **`draw` draws and advances nothing, and reads nothing but `now` and what
  `celebrate` last set.** (Widened later to include the figure on screen, for
  the receipt printer: see [the three-scene note](2026-08-12-three-more-scenes.md).)
- **Every rectangle stays inside the stage at a drift of ±3 by ±2, unless it is a
  backdrop that spans the stage at every drift.**

The second rule is the expensive one and the one that earns its keep. A secondary
display paints one frame and then repaints only when the figure changes;
`prefers-reduced-motion` never steps at all. Anything a scene caches in `step` is
frozen on both paths — so a pose stored there is a pose the static display holds
for the rest of the night while the parts derived from the clock carry on around
it, and a boolean saying "the ore is broken" is an ore that never comes back.

So the poses are not stored. The flame flicker is
`(floor(now/90) + row*3) % 3 - 1`. A drive's LED is a hash of its row, its column
and a phase counter. The swing pose is `now % 1150` bucketed into four. The ore
holds the moment it shattered, not the fact that it did, and works out the rest
from `now`. What is left in `step` is only what genuinely cannot be derived:
detecting the edge of a swing so a strike throws debris exactly once.

That is also what makes the geometry check possible, since a frame rendered at
any instant is a real frame. `checks/verify-scenes.js` reads the `SCENES`
registry out of `ui.html`, runs the real page once per scene, and asserts pixel
alignment and bounds at all five drift offsets, that every frame opens with the
background clear, that particles stay inside the envelope `stepBits` culls
against, and that no two registry entries draw the same thing. Reading the
registry rather than listing the scenes means a fifth one is checked without
anyone remembering to add it.

It has one blind spot that no amount of care inside it can fix: it takes the
scene list from the registry, so it can never ask for a name the page does not
have. A key that stops matching `Scene::key()` on the Rust side is invisible to
it — and invisible generally, because the page falls back to the mine for an
unknown name. That one is caught in `saver.rs`, which scrapes the same registry
out of the embedded page and asserts one entry per selectable scene.

## 3. What the scenes share turned out to be most of it

The four are less different than they look. Clawd is one function, and the only
argument the scenes added to it is `standing` — the jetty draws his legs itself,
hanging over the water, so it asks for a Clawd without them.

The mine and the forge share their whole swing: the same four poses, the same
1150 ms cycle, the same bob, all of it lifted into `SWING` above the divider.
Only the tool head and what it hits differ. Shovelling coal and swinging a
pickaxe are the same motion at different targets, and writing them twice would
have meant two sets of timings drifting apart.

The rack and the jetty do not swing at all, which is the reason they are in the
set. Four variations on a pickaxe would have been one scene with four skins.

What is genuinely per-scene is small: a handful of colours, a prop, an idle
motion, and the celebration. The mine shatters its ore. The forge flares for
1400 ms. A wave of green runs up the rack's bays, bottom to top, so it reads as
something rising through the machine rather than a row of lights blinking at
once. Something takes the bait and arcs out of the water on a parabola.

## 4. Accent colours, because the celebration was the only thing left to say with

The counter already flashed when the figure rose. With four scenes it flashes in
the scene's own colour — the mine's gem green, the forge's hottest flame, the
rack's terminal green, the jetty's moon. It costs one field on the scene object
and it is the only place a scene reaches above the divider.

## 5. Random is resolved once, in Rust, not in the page

`SceneChoice` is `Random` or `One(Scene)`, and only the first is a decision left
to run time. It is resolved in `main.rs` before any window exists, and the
resulting `Scene` is what every surface is built with.

Doing it in the page instead would have been fewer moving parts and wrong: the
saver builds one webview per monitor, each page would roll independently, and a
three-monitor setup would show three different scenes. Rolling it per launch in
one place also means the log can name what ran, which is the only way to find out
afterwards what was on screen.

The roll itself is `RandomState::new().build_hasher().finish()` — the hasher
Rust's `HashMap` seeds from the OS. It is a different number every time it is
constructed, which is all that is needed to pick one of four, and it costs
neither a dependency nor a stored seed. Two tests hold it to that: that it is not
a constant, and that over 4,000 rolls it reaches every scene.

## 6. Settings became a record rather than a value

The dialog stored one key. It now stores two, and the storage format changed
shape with it: `{"period":"...","scene":"..."}`, with the IPC message carrying
the same JSON rather than a delimiter.

Fields fall back individually. A `settings.json` written before scenes existed
still selects its period and takes the mine for the other half; a scene key this
build does not recognise does not discard the period beside it. That is the
behaviour that lets the format grow again without a migration.

The two sides fall back to different things, and the difference is the point.
Reading the file, an unknown field falls back to the default, because there is
nothing else to fall back to. Reading an IPC message, it falls back to **what is
currently stored**: the dialog knows what the setting was, and a newer page
naming a scene this build cannot draw should leave that setting alone rather than
reset it to the mine. Neither is reachable while the page is compiled into the
binary, which is exactly why they are worth pinning before a third field exists.

One thing the JSON needed that the old delimiter did not: **valid JSON of the
wrong shape is not a settings record.** `[]` and `"1d"` both parse, both have no
fields, and without a check that the value is an object both would have read as
"every field absent" and written a record on a message that meant nothing. Two
tests caught it, one on each side of the IPC boundary — the same bug had been
written twice, in `settings.rs` and in `config.rs`, because the parsing was
written twice.

The dialog grew a second radio group. The two are built by one factory and keyed
by which group holds focus, so the arrow keys move within a list rather than
between them, and Save carries both fields whether or not either was touched.

---

## What it cost elsewhere

**The dialog outgrew its window.** Two groups of five do not fit a fixed 440×700
at every font size, so the form scrolls and the buttons stay below it. Setting
`overflow-y` makes the element a scroll container on *both* axes, which clips
anything drawn outside the padding box — including the focus ring, which extends
`outline-offset` plus `outline-width`, 4 px, beyond the row. The container
carries 4 px of padding and −4 px of margin to give the ring room without moving
anything.

**Sitting down took three tries.** The jetty is the only scene where Clawd is not
standing, and every version of it that was wrong was wrong in a different way.

First it read as him standing behind a fence: the plank was drawn over him with
nothing in front of it. Adding a front face and dangling legs after it fixed
that and produced the second failure — the legs were two, narrow, and centred
six units left of the body, with a band of decking left between his bottom edge
and their top. Correct occlusion, since a real jetty edge does hide the thighs of
someone sitting on it, and it still read as a character cut in half.

What it takes is four things at once, and any one missing loses it:

- the plank's **top face ends at the body's bottom edge**, so his own silhouette
  hides it and no strip of decking shows under him;
- the **legs start at that same edge**, at the standing legs' own x positions and
  widths, so they line up with the body by construction rather than by eye;
- the **posts carry the deck down past the waterline**, so the plank is a
  structure rather than a line;
- and **he does not bob.** The scene had the same one-unit idle shift as the
  others, which on a figure resting against a fixed plank lifts him off it and
  reopens the seam for half of every 3.2 seconds. The water and the float carry
  the motion instead.

Its moon needed a second look too: a crescent at a resolution where the whole
moon is eight units across reads as an L. It is now two overlapping rectangles
making a rough octagon.

None of this was caught by the geometry check, which had nothing to say about any
of it: every version was perfectly aligned, in bounds, and wrong. That is the
boundary of what these checks are for.

**The sea was exactly as wide as the stage.** Which is a sixth of a second's
thought short of right: the whole scene drifts by up to three units sideways and
two down for burn-in, so several times a minute the water slid off one edge and
left a seven-unit black wedge beside it, or a four-unit strip along the bottom.
It is the only backdrop in any of the four — everything else is a prop against
black, where drift is invisible by construction — so nothing else has the
problem and nothing had ever needed the rule. The sea is now drawn oversized and
the canvas clips it, and `verify-scenes.js` gained the matching exception: a rect
may overhang only if it still spans the stage and reaches the bottom while doing
it, which a misplaced prop cannot fake.

**Three bugs on the paths nobody looks at.** A secondary display and a
reduced-motion primary both never call `step`, and three things had quietly been
relying on it:

- particles were spawned by `celebrate` but only ever culled by `step`, so on
  those paths every celebration added debris that stayed for the rest of the
  session — invisible, since every piece sits on its spawn coordinate, and
  unbounded. `spawn` now declines when motion is off.
- the mine's ore was a boolean that only `step` could clear, so the first time
  spend rose on a secondary display the ore vanished for good. It is a timestamp
  now.
- burn-in drift was only ever recomputed inside the animation loop, which runs
  once on those displays. The one surface most likely to hold a near-identical
  image all night was the one getting no protection at all.

**`build_surface` ran out of arguments.** Threading the scene through made it
eight, one past what clippy allows, which was a fair complaint — four of them
were describing the page rather than the window. They became a `Page` struct.
