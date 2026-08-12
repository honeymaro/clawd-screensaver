# Clawd Saver: Three More Scenes

Written 2026-08-12. Follows on from [the four-scene note](2026-08-11-four-scenes.md),
which is where the scene contract and the registry come from.

Seven scenes now. The three added here are a receipt printer, a parcel conveyor
and a satellite dish. What made these three the right ones, and the one place
the contract had to give.

---

## 1. They were chosen by what was missing, not by what sounded good

With four scenes in place the set had a shape, and the shape had holes:

| | mine | forge | rack | dock |
|---|---|---|---|---|
| posture | swinging | swinging | watching | sitting |
| motion | four poses | four poses | in place | backdrop flow |
| accent | gem | flame | green | moon |

Three things nothing did. Nothing translated across the stage: every moving part
either cycled in place or flowed as a backdrop. Nothing encoded the figure in the
picture, so the counter was the only thing that knew the number. And every accent
sat in the same half of the colour wheel, which meant the celebration flash
looked broadly similar whichever scene was up.

Each of the three fills one of those:

- **The receipt** puts the number in the picture. The height of the paper pile
  is the figure.
- **The parcel line** is the first scene where something travels across it. The
  jetty's fish moves too, but only once, as a celebration.
- **The uplink** is the first that aims at something off the edge, and it is the
  only cold colour in the set apart from the mine's gem.

The ideas that were passed over were passed over on the same grounds. A campfire
is the forge again. A vending machine is the rack's silhouette with a different
label. A growing plant encodes the figure, which is the good part, but it grows
upward into the amount row and its green is the rack's.

## 2. The receipt reads the figure, which the contract did not allow

The rule was that `draw` reads nothing but `now` and what `celebrate` last set.
The receipt needs the number, so the rule now reads "nothing but `now`, the
figure on screen, and what `celebrate` last set".

That is a widening rather than a hole. The point of the original rule is that a
frame drawn without a step before it must be a real frame, because a secondary
display paints once and then repaints only when the figure changes. The figure is
set by the host, not by `step`, so a scene reading it repaints exactly when it
has something new to say. Nothing about the static path gets worse.

The mapping is logarithmic:

```js
const t = Math.min(1, Math.log10(1 + cost) / Math.log10(1 + CAP));
return Math.round(t * MAX_ROWS);
```

Linear does not work here. A day and a month differ by two orders of magnitude on
this machine, so a pile scaled to fit a month is invisible for a day, and one
scaled to a day is off the top of the stage by the second week. With `CAP` at
1000, ten dollars is a third of the way up and a thousand fills it.

Eight rows at two units each, always anchored to the floor, so nothing about the
scene breaks at zero: with nothing printed yet the paper simply hangs from the
machine to the ground. Eight rather than more so the pile always tops out below
the slot, because the paper needs somewhere to fall.

**The pile alone is a still image.** It only moves when the figure does, which is
every ten seconds at best and usually not at all, and a printer that is not
printing is a scene about nothing. So the paper has to move, and the first
attempt at that was wrong in an instructive way.

That attempt fed a fold out of the slot and let its tip travel down to the pile
over 700 ms, then started the next one. Which is what really happens, and which
looks like a glitch: the visible ribbon grows and then snaps back to nothing,
once a second, forever. A sawtooth is a sawtooth however well motivated.

The fix is to move the right thing. The ribbon is a fixed length, from the slot
down to the pile, and what travels is what is printed on it: two columns of ink
marks repeating every four units, stepping down a unit every 90 ms. Because the
pattern repeats on exactly the period the offset cycles through, one line leaving
the bottom as another enters the top has no seam. That is the same trick the
conveyor's tread uses, and it is the only kind of loop that survives being
watched for an hour.

Around it, two smaller things on their own beats, so the scene never lines up
into a single pulse: a head scanning behind the machine's window (six steps for
four positions, so it turns round at each end rather than snapping back) and a
roller alternating in the slot.

**Nothing lands on the pile, and that took a third attempt to accept.** A sheet
arriving and settling onto the stack is the obvious next thing to animate, and it
cannot be done here. The pile's height is the figure, so it cannot actually grow.
Every version of an arriving sheet therefore has to take the sheet away again at
the end of the cycle, and taking it away is a pop once a second: the same
sawtooth as before, wearing a different hat. Landing it exactly on top of the
existing top sheet hides the removal but leaves the arrival popping into
existence four units up.

So the pile holds still. It is the record of what has been printed, which is a
thing that does not move; the paper is the printing, which never stops. Splitting
the scene that way is what made it read.

## 3. The parcel line is one counter and nothing else

The temptation with a conveyor is to give each parcel a position and step them.
Then the stamp needs to know when a parcel is under it, which means either
searching the parcels every frame or storing a flag, and a stored flag is frozen
on a display that never steps.

Instead there is one tick counter and everything is a function of it:

```js
const k = Math.floor(now / TICK);
const offset = (k, i) => (k * 2 + i * 10) % SPAN;   // SPAN 40, four parcels
```

Four parcels ten units apart, moving two units a tick over a span of forty, means
one parcel is at the stamp column every five ticks. So `k % 5` is the entire
rhythm of the scene: the arm is down on beat three, on its way at two and four,
and lifted otherwise. The stamp cannot drift out of phase with the parcels
because there is no second clock to drift against.

Whether a parcel already carries a mark is derived the same way. A parcel is
stamped once it is past the stamp column, so `offset >= STAMP_AT` is the whole
test, and it survives a still frame with no state at all.

**The stamp itself was a selfie stick for a while.** The first version put the
stamp column sixteen units to the right of Clawd's hand, which needs a handle
four times the length of the stamp head to reach. That is not a stamp, it is a
pole with something on the end, and no amount of detail on the head fixes the
proportions.

Two things had to change together. The stamp column moved to the parcel directly
in front of him, and the head became two and a half times the width of its grip
rather than half of it.

That in turn fixes the height of the conveyor, which is worth writing down
because it is not obvious: Clawd's hand sits between y 51 and 57, so a parcel lid
has to be below that for him to press down on one at all. A belt at a
comfortable-looking height puts the lid above his hand and he ends up stamping
upwards. It is a low conveyor because he is a short character.

**And the parcels were teleporting.** With four of them ten units apart over a
span of forty, one is always at the end of its travel, and the modulus sends it
back to the start in a single tick. On a belt long enough to show both ends that
is a box vanishing from plain sight and reappearing elsewhere, once every five
ticks, in lockstep with the stamp.

The arithmetic cannot move: the span is what sets the five-tick rhythm. So the
ends are boxed in instead. A feeder covers the position they wrap to and a chute
covers the position they wrap from, both drawn over the parcels, and the jump
happens inside one of them. It is also just what a conveyor looks like, which is
the usual sign that a fix is the right one.

## 4. The dish had to be face on

Two attempts at a dish in profile failed before the obvious one worked.

The first was two overlapping rectangles for an octagon, the trick the jetty's
moon uses. At the moon's size, eight units across, that reads as round. At twenty
by eighteen it reads as a television.

The second was an honest parabola: a dish seen from the side is a bracket, so
nine rows of two units whose left edge steps out and back, with the last two
units of each row lit. That is geometrically right and visually a hook. A
parabola in profile is a thin curved sliver, and thin curved slivers of grey on a
near-black stage do not read as anything.

Face on solves it, because the problem was never the curve. It was mass. Pointed
at the viewer the dish is the widest solid shape the right-hand side can hold, 28
units by 20, and it can be built as three concentric shells:

```js
const back = lit(2) ? SIG_L : FACE;      // 28 x 20 octagon, three rects
const mid  = lit(1) ? SIG_L : FACE_L;    // 18 x 14, painted over its middle
rect(84, 36, 12, 8, lit(0) ? SIG_L : SIG);   // the throat
```

Painted largest first, each shell covers the middle of the last, so lighting one
lights a ring rather than a disc. Cycling `lit` outwards, throat then face then
rim then a beat of nothing, is a pulse leaving the middle. The outer shell takes
three rects rather than the two the jetty's moon uses, because at this size a
two-rect octagon still has corners square enough to read as a screen on a stand;
the shells inside it are smaller and get away with fewer.

Three struts and a feed horn go on top of all of it. Without them a lit disc on a
pole is a lamp. And the throat is dim blue rather than black between pulses:
otherwise the dish is a dark disc on a dark stage for three quarters of every
second.

Nothing here goes above y 29. The amount row reaches y 24 at its tallest and both
drift together, so that clearance holds at every offset. Aiming the dish upward
instead would have put the signal straight through the counter, which is the
reason it points at the viewer rather than at the sky.

## 5. Clawd was still dressed as a miner

The hard hat and its head lamp were drawn inside `drawClawd`, which was fine
while there was one scene and quietly wrong the moment there were four. Clawd was
turning up at a server rack, on a jetty and at a printer in mining kit.

Headgear is costume, not anatomy, so it moved out into a `HAT` table and each
scene names one: the hard hat at the ore face, goggles pushed up on the brow at
the forge, a headset at the rack, a straw hat on the jetty, a print room eyeshade
at the printer, a peaked cap on the parcel line, a woolly hat at the dish.

They are told apart by silhouette rather than by colour, because at this size
colour is the first thing to go: the straw hat is wider and floppier than the
hard hat, the cap and the headset are the two asymmetric ones and asymmetric in
different directions, the visor has no crown at all, and the headset is a band
with two cups rather than a solid mass.

Every crown sits between y 26 and the top of the body at 36. The amount row
reaches y 24 and drifts with everything else, so that clearance holds for good.
The headset's boom is the one piece that hangs below the body line, and it has to
touch its cup: left floating it is a stick and a ball in the air beside his chin,
which is what the first version of it looked like.

## 6. What the scenes still share

Nothing new was added to the shared layer for these three except one helper:

```js
const idleBob = (now, phase) => (Math.floor((now + phase) / 1000) % 2 === 0 ? 0 : -1);
```

The slow shift of weight for a Clawd who is watching rather than hitting
something. It takes a phase so the three scenes that use it do not breathe in
unison, and the phase is in milliseconds and added outside the floor: added
inside, an integer phase only flips the parity, so every even value lands back
on the first scene and the argument silently does nothing.

None of the three has a `step`. Every moving part is a function of the clock, and
the only state any of them keeps is the moment its celebration started.

---

## What it cost elsewhere

**The dialog stopped fitting.** Thirteen options against a fixed 440 by 700
window: the form now scrolls, which is what the scroll container added with the
second radio group was for. It was not made taller instead, because the window
cannot be resized and one sized for the scene list would hang off the bottom of a
768-line laptop screen. Scrolling on a small screen beats a dialog whose Save
button is off the edge of it.

**The receipt was rebuilt once, and the lesson was general.** The first version
ran the paper along the ground: a two-unit strip in a single shade, which is a
painted line down the middle of a road. Thickening it, alternating the shade
every six units and adding a ridge on each fold helped and did not fix it, and
neither did moving the printer closer.

What fixed it was noticing what the scenes that work have in common. The mine is
a solid ore block, the forge a dark furnace around a fire, the rack a dark box
around its lights: every one of them is one large mass with a saturated colour
inside it. The printer had neither. It was a grey machine on a dark stage with a
thin pale ribbon.

So the bill became the mass. A fan-folded pile beside Clawd, every other sheet
sitting two units proud, in the one warm off-white nothing else on the stage
uses, and as tall as the figure. The machine shrank to a dark box on legs whose
only saturated thing is three red lamps, which is also the scene's accent. The
same reasoning is what turned the dish face on.

**The check caught the key list, as designed.** `checks/smoke-settings.js` keeps
its own copy of the option keys on purpose, as a second opinion about what the
page offers. Adding three scenes failed it immediately, which is the behaviour
that copy exists for.
