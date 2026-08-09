// Headless smoke test for saver/src/settings.html.
// Stubs just enough DOM to execute the page script and drive it the way a user
// would, so the messages it posts back to the host can be asserted without
// opening a window.
//
// The host side of the same contract is checked by config.rs's `message()`
// tests. Between them, the only untested link is WebView2's IPC bridge itself,
// which the saver already depends on to close on input.
const fs = require('fs');
const vm = require('vm');
const path = require('path');

const html = fs.readFileSync(
  path.join(__dirname, '..', 'saver', 'src', 'settings.html'), 'utf8');
const js = html.slice(html.indexOf('<script>') + 8, html.lastIndexOf('</script>'));

// The keys the page offers must be the ones settings.rs accepts. Kept here as a
// literal rather than read from the page, so a key changing on one side only
// fails instead of silently agreeing with itself.
const KEYS = ['1d', 'wtd', 'mtd', '7d', '30d'];

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
  };
  return node;
}

function makeSandbox(period) {
  const posted = [];
  const byId = { options: el(), save: el('BUTTON'), cancel: el('BUTTON') };
  const winListeners = {};

  const win = {
    CLAWD_PERIOD: period,
    ipc: { postMessage: m => posted.push(m) },
    document: {
      getElementById: id => byId[id] || el(),
      createElement: () => el(),
    },
    addEventListener(type, fn) { (winListeners[type] ||= []).push(fn); },
    console, JSON, Math, String,
  };
  win.window = win;
  win.globalThis = win;

  return {
    win, posted, byId,
    rows: () => byId.options.children,
    // `target` is what has focus. It matters for Enter: a focused button
    // activates itself, so the page must not also run the global shortcut.
    key(k, target) {
      let prevented = false;
      const ev = { key: k, target, preventDefault() { prevented = true; } };
      (winListeners.keydown || []).forEach(fn => fn(ev));
      return prevented;
    },
  };
}

function run(label, period, script) {
  const s = makeSandbox(period);
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
const selectedKey = s => {
  const at = s.rows().findIndex(r => r.classList.contains('sel'));
  if (at < 0) throw new Error('nothing is selected');
  if (s.rows().filter(r => r.classList.contains('sel')).length !== 1) {
    throw new Error('more than one option is selected');
  }
  return KEYS[at];
};

let ok = true;
console.log('settings.html headless smoke test');

ok &= run('renders one row per period', '1d', s => {
  eq(s.rows().length, KEYS.length, 'row count');
});

for (const key of KEYS) {
  ok &= run(`opens on the stored choice (${key})`, key, s => {
    eq(selectedKey(s), key, 'preselected');
  });
}

ok &= run('an unknown stored choice falls back to today', '1y', s => {
  // Matches settings.rs, which also falls back rather than showing nothing.
  eq(selectedKey(s), '1d', 'preselected');
});

ok &= run('a missing stored choice falls back to today', undefined, s => {
  eq(selectedKey(s), '1d', 'preselected');
});

// Indices go through KEYS rather than being written out, so reordering or
// adding a period changes one list instead of silently retargeting these tests.
const LAST = KEYS.length - 1;

ok &= run('clicking an option then saving posts that option', '1d', s => {
  s.rows()[LAST].click();
  eq(selectedKey(s), KEYS[LAST], 'after click');
  s.byId.save.click();
  eq(s.posted, [`save:${KEYS[LAST]}`], 'posted');
});

ok &= run('saving without touching anything keeps the stored choice', '7d', s => {
  s.byId.save.click();
  eq(s.posted, ['save:7d'], 'posted');
});

ok &= run('cancel closes without saving', '7d', s => {
  s.rows()[LAST].click();
  s.byId.cancel.click();
  eq(s.posted, ['close'], 'posted');
});

ok &= run('escape closes without saving', '7d', s => {
  s.rows()[0].click();
  s.key('Escape');
  eq(s.posted, ['close'], 'posted');
});

ok &= run('enter on an option row saves it', '1d', s => {
  s.key('ArrowDown', s.rows()[0]);
  s.key('Enter', s.rows()[1]);
  eq(s.posted, [`save:${KEYS[1]}`], 'posted');
});

// A focused button already activates itself on Enter. If the global shortcut
// ran too, Enter on Cancel would post save: first and close second, and the host
// acts on the first — so Cancel would quietly save.
ok &= run('enter on cancel closes without saving', '1d', s => {
  s.rows()[2].click();
  eq(s.key('Enter', s.byId.cancel), false, 'the shortcut must stand aside');
  eq(s.posted, [], 'nothing may be posted before the button acts');
  s.byId.cancel.click();   // what the browser does next
  eq(s.posted, ['close'], 'posted');
});

ok &= run('enter on save posts exactly one save', '1d', s => {
  s.key('Enter', s.byId.save);
  s.byId.save.click();
  eq(s.posted, ['save:1d'], 'posted');
});

// A button's own keyboard handling owns the arrow keys too. Reaching into the
// list from there would change what a following Enter stores, without the
// selection the user is looking at ever having been touched deliberately.
ok &= run('arrows on a focused button leave the selection alone', '7d', s => {
  for (const key of ['ArrowDown', 'ArrowUp']) {
    for (const button of [s.byId.save, s.byId.cancel]) {
      eq(s.key(key, button), false, `${key} on a button must stand aside`);
      eq(selectedKey(s), '7d', `${key} on a button moved the selection`);
    }
  }
  eq(s.posted, [], 'posted');
});

ok &= run('arrow keys wrap in both directions', '1d', s => {
  eq(s.key('ArrowUp'), true, 'ArrowUp should be handled');
  eq(selectedKey(s), KEYS[LAST], 'wrapped backwards past the first');
  s.key('ArrowDown');
  eq(selectedKey(s), KEYS[0], 'wrapped forwards past the last');
});

ok &= run('unrelated keys are left alone', '1d', s => {
  eq(s.key('Tab'), false, 'Tab must not be swallowed');
  eq(s.posted, [], 'posted');
});

// Without a host the page cannot learn the stored choice, so it shows Today.
// Saving then would quietly replace a stored 30d with 1d. The same missing host
// means there is nothing to post to, which is what makes that safe — pinned here
// because the safety comes from a different line than the bug would.
ok &= run('with no host, nothing can be saved', undefined, s => {
  delete s.win.ipc;
  eq(selectedKey(s), '1d', 'falls back to today');
  s.rows()[LAST].click();
  s.byId.save.click();
  s.key('Enter', s.rows()[LAST]);
  eq(s.posted, [], 'nothing may be posted');
});

ok &= run('every offered key is one the host accepts', '1d', s => {
  // A key the page can post but settings.rs cannot parse is an option that
  // silently behaves as Cancel.
  KEYS.forEach((key, i) => {
    const fresh = makeSandbox('1d');
    vm.createContext(fresh.win);
    vm.runInContext(js, fresh.win, { filename: 'settings.html' });
    fresh.rows()[i].click();
    fresh.byId.save.click();
    eq(fresh.posted, [`save:${key}`], `option ${i}`);
  });
});

console.log(ok ? '\nALL SETTINGS INTERACTIONS BEHAVE' : '\nFAILURES ABOVE');
process.exit(ok ? 0 : 1);
