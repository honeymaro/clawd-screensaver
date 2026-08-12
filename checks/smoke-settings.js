// Headless smoke test for saver/src/settings.html.
// Stubs just enough DOM to execute the page script and drive it the way a user
// would, so the messages it posts back to the host can be asserted without
// opening a window.
//
// The host side of the same contract is checked by config.rs's `message()`
// tests, which read the page's own key lists rather than repeating them.
// Between them, the only untested link is WebView2's IPC bridge itself, which
// the saver already depends on to close on input.
const fs = require('fs');
const vm = require('vm');
const path = require('path');

const html = fs.readFileSync(
  path.join(__dirname, '..', 'saver', 'src', 'settings.html'), 'utf8');
const js = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));

// The keys each list offers, kept here as literals rather than read from the
// page: this file is the second opinion, so agreeing by construction would
// defeat it. config.rs checks the page against the Rust enums; this checks it
// against what a reader expects to see.
const PERIODS = ['1d', 'wtd', 'mtd', '7d', '30d'];
const SCENES = ['random', 'mine', 'forge', 'rack', 'dock', 'printer', 'belt',
                'uplink', 'dojo'];

function el(tagName = 'DIV') {
  const node = {
    tagName,
    className: '', tabIndex: -1, textContent: '', innerHTML: '',
    children: [], attrs: {}, classes: new Set(), listeners: {}, focused: 0,
    classList: {
      toggle(name, on) { on ? node.classes.add(name) : node.classes.delete(name); },
      contains: name => node.classes.has(name),
    },
    setAttribute(k, v) { node.attrs[k] = v; },
    getAttribute(k) { return node.attrs[k]; },
    appendChild(c) { node.children.push(c); },
    addEventListener(type, fn) { (node.listeners[type] ||= []).push(fn); },
    focus() { node.focused++; },
    // The page writes a fixed innerHTML and then reaches into it by class.
    // Handing back a fabricated element for any selector would let a class
    // renamed on one side only pass here while the real page throws on null and
    // renders no rows at all — the exact typo this file exists to catch.
    querySelector(sel) {
      if (!sel.startsWith('.')) throw new Error(`stub handles class selectors only, got "${sel}"`);
      const token = sel.slice(1);
      if (!new RegExp(`class="[^"]*\\b${token}\\b[^"]*"`).test(node.innerHTML)) {
        throw new Error(`querySelector("${sel}") matches nothing in ${JSON.stringify(node.innerHTML)}`);
      }
      return (node.spans ||= {})[sel] ||= el();
    },
    click() { (node.listeners.click || []).forEach(fn => fn({})); },
    scrollIntoView() { node.scrolledTo++; },
    scrolledTo: 0,
  };
  return node;
}

// Every id the markup declares, with the tag that carries it. Read from the
// file rather than listed here for the same reason querySelector above is
// strict: a stub that hands back a fresh element for any id lets the page ask
// for one the markup does not have. In a browser that is `null`, a throw inside
// the IIFE at the first `setAttribute`, and a dialog with no rows, no buttons
// and no keyboard — while every test here still passes.
const MARKUP_IDS = new Map(
  [...html.matchAll(/<(\w+)\b[^>]*\bid="([^"]+)"/g)].map(m => [m[2], m[1].toUpperCase()]));

function makeSandbox(period, scene) {
  const posted = [];
  const byId = {};
  for (const [id, tag] of MARKUP_IDS) byId[id] = el(tag);
  const winListeners = {};

  const win = {
    CLAWD_PERIOD: period,
    CLAWD_SCENE: scene,
    ipc: { postMessage: m => posted.push(m) },
    document: {
      getElementById: id => {
        if (!(id in byId)) throw new Error(`settings.html declares no id="${id}"`);
        return byId[id];
      },
      createElement: () => el(),
    },
    addEventListener(type, fn) { (winListeners[type] ||= []).push(fn); },
    console, JSON, Math, String,
  };
  win.window = win;
  win.globalThis = win;

  return {
    win, posted, byId,
    periodRows: () => byId.periods.children,
    sceneRows: () => byId.scenes.children,
    // `target` is what has focus. It matters for Enter and the arrows: a
    // focused button acts for itself, and arrows move within one group only.
    key(k, target) {
      let prevented = false;
      const ev = { key: k, target, preventDefault() { prevented = true; } };
      (winListeners.keydown || []).forEach(fn => fn(ev));
      return prevented;
    },
  };
}

function run(label, period, scene, script) {
  const s = makeSandbox(period, scene);
  vm.createContext(s.win);
  try {
    vm.runInContext(js, s.win, { filename: 'settings.html' });
    script(s);
    console.log(`  PASS  ${label}`);
    return true;
  } catch (e) {
    console.log(`  FAIL  ${label}\n        ${e.message}`);
    return false;
  }
}

const eq = (got, want, what) => {
  if (JSON.stringify(got) !== JSON.stringify(want)) {
    throw new Error(`${what}: expected ${JSON.stringify(want)}, got ${JSON.stringify(got)}`);
  }
};
const chosen = (rows, keys) => {
  const on = rows.filter(r => r.classList.contains('sel'));
  if (on.length !== 1) throw new Error(`${on.length} rows selected, expected exactly 1`);
  return keys[rows.indexOf(on[0])];
};
const saved = s => {
  eq(s.posted.length, 1, 'exactly one message');
  const m = s.posted[0];
  if (!m.startsWith('save:')) throw new Error(`expected a save, got ${m}`);
  return JSON.parse(m.slice(5));
};

let ok = true;
console.log('settings.html headless smoke test');

ok &= run('renders one row per option in each group', '1d', 'mine', s => {
  eq(s.periodRows().length, PERIODS.length, 'period rows');
  eq(s.sceneRows().length, SCENES.length, 'scene rows');
});

for (const key of PERIODS) {
  ok &= run(`opens on the stored period (${key})`, key, 'mine', s => {
    eq(chosen(s.periodRows(), PERIODS), key, 'preselected period');
  });
}
for (const key of SCENES) {
  ok &= run(`opens on the stored scene (${key})`, '1d', key, s => {
    eq(chosen(s.sceneRows(), SCENES), key, 'preselected scene');
  });
}

// Fourteen options do not fit the window, and the scene group is the half that
// starts below the fold. A stored scene nobody can see reads as a lost setting,
// and what brings it into view is being focused: `focus()` scrolls to its own
// element. So the assertion is about focus, which this stub can see, rather
// than about scroll position, which it cannot.
for (const key of SCENES) {
  ok &= run(`opening on ${key} puts focus on that row`, '1d', key, s => {
    const row = s.sceneRows()[SCENES.indexOf(key)];
    eq(row.focused, 1, 'the stored scene row was not the one focused');
    const strays = [...s.periodRows(), ...s.sceneRows()].filter(r => r !== row && r.focused);
    eq(strays.length, 0, 'something else was focused as well, and focus scrolls');
  });
}

ok &= run('unknown stored values fall back the way the saver does', '1y', 'volcano', s => {
  eq(chosen(s.periodRows(), PERIODS), '1d', 'period');
  eq(chosen(s.sceneRows(), SCENES), 'mine', 'scene');
});

ok &= run('missing stored values fall back too', undefined, undefined, s => {
  eq(chosen(s.periodRows(), PERIODS), '1d', 'period');
  eq(chosen(s.sceneRows(), SCENES), 'mine', 'scene');
});

// Every row, not a sample of them. A click handler bound to the wrong index —
// `select(i === 3 ? 2 : i)` — is exactly the mistake two spot-checks walk past,
// and it means clicking one option stores another.
for (let i = 0; i < PERIODS.length; i++) {
  ok &= run(`clicking period row ${i} stores ${PERIODS[i]}`, '1d', 'mine', s => {
    s.periodRows()[i].click();
    s.byId.save.click();
    eq(saved(s), { period: PERIODS[i], scene: 'mine' }, 'payload');
  });
}
for (let i = 0; i < SCENES.length; i++) {
  ok &= run(`clicking scene row ${i} stores ${SCENES[i]}`, '1d', 'mine', s => {
    s.sceneRows()[i].click();
    s.byId.save.click();
    eq(saved(s), { period: '1d', scene: SCENES[i] }, 'payload');
  });
}

ok &= run('saving sends both fields, not just the one that changed', '1d', 'mine', s => {
  s.sceneRows()[SCENES.indexOf('dock')].click();
  s.byId.save.click();
  eq(saved(s), { period: '1d', scene: 'dock' }, 'payload');
});

ok &= run('saving untouched keeps what was stored', '30d', 'rack', s => {
  s.byId.save.click();
  eq(saved(s), { period: '30d', scene: 'rack' }, 'payload');
});

ok &= run('the two groups do not disturb each other', '1d', 'mine', s => {
  s.periodRows()[PERIODS.indexOf('mtd')].click();
  eq(chosen(s.sceneRows(), SCENES), 'mine', 'scene moved when a period was clicked');
  s.sceneRows()[SCENES.indexOf('forge')].click();
  eq(chosen(s.periodRows(), PERIODS), 'mtd', 'period moved when a scene was clicked');
  s.byId.save.click();
  eq(saved(s), { period: 'mtd', scene: 'forge' }, 'payload');
});

ok &= run('cancel closes without saving', '7d', 'dock', s => {
  s.periodRows()[0].click();
  s.byId.cancel.click();
  eq(s.posted, ['close'], 'posted');
});

ok &= run('escape closes without saving', '7d', 'dock', s => {
  s.key('Escape');
  eq(s.posted, ['close'], 'posted');
});

ok &= run('enter on an option row saves', '1d', 'mine', s => {
  s.key('Enter', s.periodRows()[0]);
  eq(saved(s), { period: '1d', scene: 'mine' }, 'payload');
});

// A focused button already activates itself on Enter. If the global shortcut
// ran too, Enter on Cancel would post save: first and close second, and the host
// acts on the first — so Cancel would quietly save.
ok &= run('enter on cancel closes without saving', '1d', 'mine', s => {
  eq(s.key('Enter', s.byId.cancel), false, 'the shortcut must stand aside');
  eq(s.posted, [], 'nothing may be posted before the button acts');
  s.byId.cancel.click();
  eq(s.posted, ['close'], 'posted');
});

ok &= run('arrows move within the group that has focus', '1d', 'mine', s => {
  s.key('ArrowDown', s.periodRows()[0]);
  eq(chosen(s.periodRows(), PERIODS), PERIODS[1], 'period moved');
  eq(chosen(s.sceneRows(), SCENES), 'mine', 'scene must not move with it');

  s.key('ArrowDown', s.sceneRows()[0]);
  eq(chosen(s.sceneRows(), SCENES), SCENES[1], 'scene moved');
  eq(chosen(s.periodRows(), PERIODS), PERIODS[1], 'period must not move with it');
});

ok &= run('arrows wrap within their own group', '1d', 'mine', s => {
  s.key('ArrowUp', s.periodRows()[0]);
  eq(chosen(s.periodRows(), PERIODS), PERIODS[PERIODS.length - 1], 'wrapped backwards');
  s.key('ArrowDown', s.periodRows()[PERIODS.length - 1]);
  eq(chosen(s.periodRows(), PERIODS), PERIODS[0], 'wrapped forwards');
});

ok &= run('arrows on a focused button leave both groups alone', '7d', 'rack', s => {
  for (const k of ['ArrowDown', 'ArrowUp']) {
    for (const button of [s.byId.save, s.byId.cancel]) {
      eq(s.key(k, button), false, `${k} on a button must stand aside`);
    }
  }
  eq(chosen(s.periodRows(), PERIODS), '7d', 'period moved');
  eq(chosen(s.sceneRows(), SCENES), 'rack', 'scene moved');
  eq(s.posted, [], 'posted');
});

ok &= run('unrelated keys are left alone', '1d', 'mine', s => {
  eq(s.key('Tab'), false, 'Tab must not be swallowed');
  eq(s.posted, [], 'posted');
});

// Without a host the page cannot learn the stored choices, so it shows the
// defaults. Saving then would quietly replace them. The same missing host means
// there is nothing to post to, which is what makes that safe — pinned here
// because the safety comes from a different line than the bug would.
ok &= run('with no host, nothing can be saved', undefined, undefined, s => {
  delete s.win.ipc;
  s.sceneRows()[SCENES.length - 1].click();
  s.byId.save.click();
  s.key('Enter', s.periodRows()[0]);
  eq(s.posted, [], 'nothing may be posted');
});

console.log(ok ? '\nALL SETTINGS INTERACTIONS BEHAVE' : '\nFAILURES ABOVE');
process.exit(ok ? 0 : 1);
