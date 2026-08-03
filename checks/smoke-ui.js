// Headless smoke test for saver/src/ui.html.
// Stubs just enough DOM to actually execute the page script and drive it
// through every usage state, so runtime errors surface without opening a window.
const fs = require('fs');
const vm = require('vm');
const path = require('path');

const html = fs.readFileSync(
  path.join(__dirname, '..', 'saver', 'src', 'ui.html'), 'utf8');
const js = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));

function makeSandbox(staticMode, seed) {
  let rafCb = null;
  const draws = [];
  let fill = null;
  const ctx = {
    set fillStyle(v) { fill = v; },
    get fillStyle() { return fill; },
    fillRect(x, y, w, h) { draws.push([x, y, w, h, fill]); },
    clearRect() {},
    imageSmoothingEnabled: true,
  };
  const canvas = { width: 0, height: 0, style: {}, getContext: () => ctx };
  let t = 0;

  const win = {
    innerWidth: 1920,
    innerHeight: 1200,
    devicePixelRatio: 1,
    matchMedia: () => ({ matches: false }),
    addEventListener: () => {},
    requestAnimationFrame: (cb) => { rafCb = cb; return 1; },
    performance: { now: () => t },
    document: { getElementById: () => canvas },
    Math, JSON, console,
  };
  if (staticMode) win.CLAWD_STATIC = true;
  if (seed) win.CLAWD_SEED = seed;
  win.window = win;
  win.globalThis = win;

  return {
    win,
    draws,
    tick(ms) { t += ms; if (rafCb) { const cb = rafCb; rafCb = null; cb(t); } },
    setTime(ms) { t = ms; },
  };
}

function run(label, staticMode, seed, script) {
  const s = makeSandbox(staticMode, seed);
  vm.createContext(s.win);
  try {
    vm.runInContext(js, s.win, { filename: 'ui.html' });
    script(s);
    const rects = s.draws.length;
    const bad = s.draws.filter(r => r.slice(0, 4).some(v => !Number.isFinite(v)));
    if (bad.length) throw new Error(`${bad.length} rect(s) with non-finite coords, e.g. ${bad[0]}`);
    console.log(`  PASS  ${label.padEnd(42)} ${rects} rects drawn`);
    return true;
  } catch (e) {
    console.log(`  FAIL  ${label.padEnd(42)} ${e.message}`);
    return false;
  }
}

let ok = true;
console.log('ui.html headless smoke test');

ok &= run('cold start, no seed (error state)', false, null, s => {
  for (let i = 0; i < 8; i++) s.tick(140);
});

ok &= run('seeded from cache (stale state)', false, { cost: 33.47, state: 'stale' }, s => {
  for (let i = 0; i < 8; i++) s.tick(140);
});

ok &= run('fresh usage pushed by host', false, { cost: 33.47, state: 'stale' }, s => {
  s.tick(200);
  s.win.CLAWD_USAGE({ cost: 33.47, state: 'ok' });
  for (let i = 0; i < 8; i++) s.tick(140);
});

ok &= run('spend increases -> celebration', false, { cost: 10, state: 'ok' }, s => {
  s.tick(200);
  s.win.CLAWD_USAGE({ cost: 42.5, state: 'ok' });
  for (let i = 0; i < 20; i++) s.tick(140);
});

// The whole point of the loading state is that it visibly moves. A static
// "$--.--" is what made the first launch of each day look broken, so assert the
// highlight actually travels rather than just that the frames render.
ok &= run('new day: loading light must travel', false, null, s => {
  const AMOUNT = '#F0EEE6';
  s.tick(200);
  s.win.CLAWD_USAGE({ cost: null, state: 'loading' });

  const positions = new Set();
  for (let i = 0; i < 30; i++) {
    const before = s.draws.length;
    s.tick(140);
    const bright = s.draws.slice(before).filter(r => r[4] === AMOUNT).map(r => r[0]);
    if (bright.length) positions.add(Math.min(...bright));
  }
  if (positions.size < 2) {
    throw new Error(`loading highlight never moved (${positions.size} position(s): ${[...positions]})`);
  }

  s.win.CLAWD_USAGE({ cost: 72.99, state: 'ok' });
  for (let i = 0; i < 8; i++) s.tick(140);
});

ok &= run('refresh over an existing figure', false, { cost: 33.47, state: 'ok' }, s => {
  s.tick(200);
  s.win.CLAWD_USAGE({ cost: 33.47, state: 'loading' });
  for (let i = 0; i < 8; i++) s.tick(140);
  s.win.CLAWD_USAGE({ cost: 33.47, state: 'ok' });
  for (let i = 0; i < 8; i++) s.tick(140);
});

ok &= run('fetch fails after success (null cost)', false, { cost: 10, state: 'ok' }, s => {
  s.tick(200);
  s.win.CLAWD_USAGE({ cost: null, state: 'error' });
  for (let i = 0; i < 8; i++) s.tick(140);
});

ok &= run('static mode paints once', true, { cost: 5.5, state: 'ok' }, s => {
  s.tick(140);
  const after = s.draws.length;
  s.tick(140);
  if (s.draws.length !== after) throw new Error('static mode kept animating');
  s.win.CLAWD_USAGE({ cost: 6.5, state: 'ok' });
  if (s.draws.length === after) throw new Error('static mode ignored a usage update');
});

ok &= run('long run reaches drift + full mine cycles', false, { cost: 1, state: 'ok' }, s => {
  for (let i = 0; i < 400; i++) s.tick(120);   // ~48s of animation
});

ok &= run('four-figure amount', false, { cost: 1234.56, state: 'ok' }, s => {
  for (let i = 0; i < 8; i++) s.tick(140);
});

console.log(ok ? '\nALL UI STATES EXECUTE CLEANLY' : '\nFAILURES ABOVE');
process.exit(ok ? 0 : 1);
