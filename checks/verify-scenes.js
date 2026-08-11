// Geometry check for every scene in saver/src/ui.html.
//
// verify-ui.js re-derives the stage and the amount row independently, because
// that arithmetic is subtle and does not grow. This one takes the opposite
// approach on purpose: it runs the real page and inspects what it actually
// draws. Scenes are broad rather than subtle, and there will be more of them —
// a second implementation of each would be four times the work and would rot.
//
// What it asserts, for every scene in the registry, over a long run and at
// every corner of the burn-in drift:
//   - the first rectangle of every frame is the background clear
//   - every rectangle lands on whole device pixels
//   - every rectangle stays inside the stage, unless it is a backdrop
//   - every particle stays inside the envelope stepBits culls against
//   - no two registry entries draw the same thing
//
// Two things it cannot see, both by construction. It takes the scene list from
// the page's own registry, so it can never ask for a name the page does not
// have — a key that stops matching `Scene::key()` on the Rust side is invisible
// here and is caught by a test in saver.rs instead. And it identifies particles
// by their size, so scenery that happens to be 1.5 x 1.5 inherits their much
// looser bounds.
const fs = require('fs');
const vm = require('vm');
const path = require('path');

const UI = path.join(__dirname, '..', 'saver', 'src', 'ui.html');
const html = fs.readFileSync(UI, 'utf8');
const js = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));

// Read the stage from the page rather than restating it: this file is not the
// independent second opinion, so agreeing with the page is the point. Each of
// these throws rather than falling back, because a constant that has moved is
// the moment to stop, not to check the wrong thing.
const number = (label, re) => {
  const m = re.exec(html);
  if (!m) throw new Error(`ui.html no longer declares ${label}`);
  return m.slice(1).map(Number);
};
const [W, H] = number('"const W = ..., H = ...;"', /const W = (\d+), H = (\d+);/);
const [, DY] = number('"const DX = ..., DY = ...;"', /const DX = (\d+), DY = (\d+);/);
const [BASE_OFF] = number('"const BASE = ... + DY;"', /const BASE = (\d+) \+ DY;/);
const BASE = BASE_OFF + DY;

// The scenes the page offers, taken from its own registry. Read with a brace
// counter rather than a regex so a scene built by a call with an object
// argument cannot truncate the list — a half-read registry would silently check
// fewer scenes, or invent a name that then falls back to the mine.
function registryKeys() {
  const at = html.indexOf('const SCENES = {');
  if (at < 0) throw new Error('ui.html no longer declares "const SCENES = { ... }"');
  const open = html.indexOf('{', at);
  let depth = 0, end = -1;
  for (let i = open; i < html.length; i++) {
    if (html[i] === '{') depth++;
    else if (html[i] === '}' && --depth === 0) { end = i; break; }
  }
  if (end < 0) throw new Error('ui.html\'s SCENES registry is unterminated');
  const keys = [...html.slice(open + 1, end).matchAll(/(?:^|,)\s*([A-Za-z_$][\w$]*)\s*:/g)]
    .map(m => m[1]);
  if (!keys.length) throw new Error('ui.html\'s SCENES registry has no entries');
  if (new Set(keys).size !== keys.length) throw new Error(`duplicate scene keys: ${keys}`);
  return keys;
}
const SCENES = registryKeys();

// Worst-case drift is the extreme corner of the random walk, matching the list
// verify-ui.js checks the stage against.
const DRIFTS = [[0, 0], [3, 2], [-3, -2], [3, -2], [-3, 2]];

// Particles are the only thing drawn at exactly 1.5 x 1.5. They are thrown with
// random velocities and legitimately leave the stage, so they are held to the
// envelope `stepBits` culls against rather than to the stage — plus a few units
// of slack, because a particle is drawn before the step that culls it. This
// cannot tell a particle from scenery that happens to be 1.5 x 1.5; what it can
// do is stop debris from being flung somewhere absurd.
const isParticle = (w, h) => w === 1.5 && h === 1.5;
const SLACK = 8;

function run(sceneName) {
  const problems = [];
  let rects = 0, particles = 0, frames = 0;
  let rafCb = null, t = 0, fill = null;
  let expectClear = false;
  let fingerprint = 0;
  // Filled in from the page's own sizing once it has run. Nothing draws before
  // then: the page's last two statements are resize() and a rAF request, and
  // the callback is held rather than called.
  let S = 0;

  // Fixed so drift stays at zero and every drift offset can be applied by hand
  // below; a page that drifted on its own would only ever test one corner.
  const rand = () => 0.5;

  const note = msg => { if (problems.length < 4000) problems.push(msg); };

  const ctx = {
    set fillStyle(v) { fill = v; },
    get fillStyle() { return fill; },
    fillRect(px, py, pw, ph) {
      // Back out of device pixels into stage units. The page has drift zero
      // here, so these are the undrifted coordinates.
      const x = px / S, y = py / S, w = pw / S, h = ph / S;

      // The frame opens with a clear that covers the stage exactly and does not
      // go through rect(), so it carries no drift and cannot be judged by the
      // same rule. Identified by being first rather than by its shape: a scene
      // drawing its own stage-sized rectangle would otherwise be waved through
      // on the one drift offset where the two look alike.
      if (expectClear) {
        expectClear = false;
        if (x !== 0 || y !== 0 || w !== W || h !== H) {
          note(`${sceneName}: frame opens with [${x},${y} ${w}x${h}], not the clear`);
        }
        return;
      }

      rects++;
      // Cheap order-sensitive hash, enough to tell two scenes apart.
      for (const v of [x, y, w, h, fill]) {
        fingerprint = (Math.imul(fingerprint, 31) + String(v).length + (+v || 0)) | 0;
      }

      const particle = isParticle(w, h);
      if (particle) particles++;

      for (const [dx, dy] of DRIFTS) {
        const v = [(x + dx) * S, (y + dy) * S, w * S, h * S];
        if (!v.every(Number.isInteger)) {
          note(`${sceneName}: non-integer device pixels ${v} at t=${t}`);
        }
        const left = x + dx, top = y + dy, right = left + w, bottom = top + h;

        if (particle) {
          // stepBits culls on b.y > BASE || b.x < -4 || b.x > W + 4.
          if (left < -4 - SLACK || left > W + 4 + SLACK || top > BASE + SLACK) {
            note(`${sceneName}: particle outside the cull envelope [${left},${top}] at t=${t}`);
          }
          continue;
        }

        // A backdrop may overhang, a misplaced prop may not, and the difference
        // is that a backdrop still covers the stage while doing it. Nothing
        // narrower than the stage or stopping short of the bottom qualifies.
        const backdrop = left <= 0 && right >= W && bottom >= H;
        if (!backdrop && (left < 0 || top < 0 || right > W || bottom > H)) {
          note(`${sceneName}: out of bounds [${left},${top} ${w}x${h}] drift(${dx},${dy}) at t=${t}`);
        }
      }
    },
    clearRect() {},
    imageSmoothingEnabled: true,
  };
  const canvas = { width: 0, height: 0, style: {}, getContext: () => ctx };

  const win = {
    innerWidth: 1920,
    innerHeight: 1200,
    devicePixelRatio: 1,
    matchMedia: () => ({ matches: false }),
    addEventListener: () => {},
    requestAnimationFrame: (cb) => { rafCb = cb; return 1; },
    performance: { now: () => t },
    document: { getElementById: () => canvas },
    Math: Object.create(Math, { random: { value: rand } }),
    JSON, console,
    CLAWD_SCENE: sceneName,
    CLAWD_SEED: { cost: 1234.56, state: 'ok' },
  };
  win.window = win;
  win.globalThis = win;

  vm.createContext(win);
  vm.runInContext(js, win, { filename: 'ui.html' });

  // Taken from the page's own sizing rather than assumed: everything above
  // divides by it, so a wrong S would quietly corrupt every coordinate.
  S = canvas.width / W;
  if (!Number.isInteger(S) || S % 2 !== 0) {
    throw new Error(`resize() produced S = ${S}, which is not an even integer`);
  }

  const tick = (ms) => {
    t += ms;
    if (rafCb) { const cb = rafCb; rafCb = null; expectClear = true; frames++; cb(t); }
  };

  // Long enough to pass through every pose, several idle cycles and a respawn.
  for (let i = 0; i < 600; i++) tick(100);
  // Then the celebration, which is the branch that draws the most.
  let cost = 1234.56;
  for (let n = 0; n < 3; n++) {
    cost += 5;
    win.CLAWD_USAGE({ cost, state: 'ok' });
    for (let i = 0; i < 60; i++) tick(60);
  }
  // And the two states that change the amount row's own geometry.
  win.CLAWD_USAGE({ cost: null, state: 'loading' });
  for (let i = 0; i < 20; i++) tick(120);
  win.CLAWD_USAGE({ cost: 98765.43, state: 'stale' });
  for (let i = 0; i < 20; i++) tick(120);

  return { problems, rects, particles, frames, fingerprint };
}

let bad = 0;
const seen = new Map();
console.log(`ui.html scene geometry (${SCENES.length} scene(s), ${DRIFTS.length} drift offsets)`);
for (const name of SCENES) {
  let r;
  try {
    r = run(name);
  } catch (e) {
    console.log(`  FAIL  ${name.padEnd(9)} threw: ${e.message}`);
    bad++;
    continue;
  }
  if (r.problems.length) {
    bad++;
    console.log(`  FAIL  ${name.padEnd(9)} ${r.problems.length} problem(s)`);
    for (const p of [...new Set(r.problems)].slice(0, 6)) console.log(`          ${p}`);
    continue;
  }
  // Two keys wired to the same factory is a copy-paste that costs a user one of
  // the options they were offered, and nothing else here would notice.
  const twin = seen.get(r.fingerprint);
  if (twin) {
    bad++;
    console.log(`  FAIL  ${name.padEnd(9)} draws exactly what "${twin}" draws`);
    continue;
  }
  seen.set(r.fingerprint, name);
  console.log(`  PASS  ${name.padEnd(9)} ${r.rects} rects (${r.particles} particle) over ${r.frames} frames`);
}

console.log(bad ? '\nSCENE GEOMETRY PROBLEMS' : '\nALL SCENES PIXEL-ALIGNED AND IN BOUNDS');
process.exit(bad ? 1 : 0);
