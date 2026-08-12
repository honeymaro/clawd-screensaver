// Geometry check for saver/src/ui.html: every rect must land on whole device
// pixels and stay inside the stage, for every pose, every drift offset and the
// widest amount string we expect to render.
//
// The geometry below is re-derived here rather than read from the page on
// purpose — an independent second implementation is what catches arithmetic the
// page would happily render wrong. What must not drift is the *data* the two
// copies start from, so assertMirrorsUi() reads those values back out of
// ui.html and refuses to run if they have changed on one side only.
const fs = require('fs');
const path = require('path');
const UI = fs.readFileSync(path.join(__dirname, '..', 'saver', 'src', 'ui.html'), 'utf8');

/// Pulls a balanced `{...}` or `[...]` literal out of the page source.
function literalFromUi(name, open, close) {
  const decl = UI.indexOf(`const ${name} = ${open}`);
  if (decl < 0) throw new Error(`ui.html no longer declares "const ${name} = ${open}"`);
  let depth = 0, i = UI.indexOf(open, decl), start = i;
  for (; i < UI.length; i++) {
    if (UI[i] === open) depth++;
    else if (UI[i] === close && --depth === 0) break;
  }
  return Function(`return ${UI.slice(start, i + 1)}`)();
}

function assertMirrorsUi(font, poses) {
  // Exact source lines, so a changed constant fails loudly instead of leaving
  // this file validating a layout the page no longer uses.
  for (const line of [
    'const W = 112, H = 74;',
    'const DX = 13, DY = 19;',
    'const OX = 4 + DX, OY = -1 + DY;',
    'const BASE = 47 + DY;',
    'const RX = 64 + DX, RY = 29 + DY, CELL = 3;',
    'const AMOUNT_MAX_W = 96;',
    // The mine's hat, pinned because this file re-derives it from DX and DY
    // while the page now writes it out in stage units. Nothing else would
    // notice the two drifting apart.
    'rect(19, 33 + bob, 44, 3, HELMET);',
    'rect(25, 27 + bob, 32, 6, HELMET);',
    'rect(36, 28 + bob, 10, 5, LAMP_BX);',
    'rect(38, 29 + bob, 6, 3, LAMP);',
  ]) {
    if (!UI.includes(line)) {
      throw new Error(`ui.html changed: "${line}" is gone — update verify-ui.js to match`);
    }
  }
  for (const [name, mine] of [['FONT', font], ['SWING', poses]]) {
    const theirs = literalFromUi(name, name === 'FONT' ? '{' : '[', name === 'FONT' ? '}' : ']');
    if (JSON.stringify(theirs) !== JSON.stringify(mine)) {
      throw new Error(`${name} differs between ui.html and verify-ui.js:\n  ui.html: ${JSON.stringify(theirs)}\n  here:    ${JSON.stringify(mine)}`);
    }
  }
}

const W = 112, H = 74, DX = 13, DY = 19;
const OX = 4 + DX, OY = -1 + DY, BASE = 47 + DY;
const RX = 64 + DX, RY = 29 + DY;
const AMOUNT_MAX_W = 96;
const S = 14; // even, as resize() guarantees

// `SWING` in ui.html, shared there by the mine and the forge. The poses are the
// mine's, which is the scene this file re-derives.
const POSES = [
  { step: [3, -3], armDY: -4.5 },
  { step: [3, -1.5], armDY: -3 },
  { step: [3, 0], armDY: 0 },
  { step: [3, -1], armDY: -1.5 },
];
const FONT = {
  '0': ['111','101','101','101','111'], '1': ['010','110','010','010','111'],
  '2': ['111','001','111','100','111'], '3': ['111','001','111','001','111'],
  '4': ['101','101','111','001','001'], '5': ['111','100','111','001','111'],
  '6': ['111','100','111','101','111'], '7': ['111','001','001','001','001'],
  '8': ['111','101','111','101','111'], '9': ['111','101','111','001','111'],
  '$': ['010','111','110','111','011','111','010'], '.': ['0','0','0','0','1'],
  '-': ['000','000','111','000','000'],
};

assertMirrorsUi(FONT, POSES);

const problems = [];
let minX = 1e9, maxX = -1e9, minY = 1e9, maxY = -1e9;

function chk(tag, x, y, w, h, dx, dy) {
  const v = [(x + dx) * S, (y + dy) * S, w * S, h * S];
  if (!v.every(Number.isInteger)) problems.push(`${tag}: non-integer ${v}`);
  if ((x + dx) < 0 || (y + dy) < 0 || (x + dx + w) > W || (y + dy + h) > H) {
    problems.push(`${tag}: out of bounds [${x + dx},${y + dy} ${w}x${h}] drift(${dx},${dy})`);
  }
  minX = Math.min(minX, x + dx); maxX = Math.max(maxX, x + dx + w);
  minY = Math.min(minY, y + dy); maxY = Math.max(maxY, y + dy + h);
}

function textWidth(t, s) {
  let w = 0;
  for (const ch of t) w += ((FONT[ch] ? FONT[ch][0].length : 3) + 1) * s;
  return w - s;
}

// Unlike the geometry above, these two are copied from ui.html rather than
// re-derived, so on their own they only catch the two files disagreeing. The
// real protection against a row that overflows is chk() below, which checks the
// resulting boxes against the stage independently of how the scale was picked —
// a wrong AMOUNT_MAX_W that let the row run off the edge still fails there. What
// a copy cannot catch is the two files being wrong in the same direction, which
// is why the expected-scale table further down states its answers outright.
function amountScale(t) {
  for (const s of [3, 2]) if (textWidth(t, s) <= AMOUNT_MAX_W) return s;
  return 1;
}
const amountY = s => 13.5 - 2.5 * s;

// Worst-case drift is the extreme corner of the random walk.
const DRIFTS = [[0, 0], [3, 2], [-3, -2], [3, -2], [-3, 2]];

for (const [dx, dy] of DRIFTS) {
  for (const [pi, p] of POSES.entries()) {
    const bob = pi === 2 ? 1 : (pi === 0 ? -1 : 0);
    const lift = bob < 0 ? 2 : 0;
    const tag = `d(${dx},${dy})p${pi}`;

    chk(`${tag} shadowC`, 12 + DX + lift, BASE, 32 - lift * 2, 1.5, dx, dy);
    chk(`${tag} shadowR`, RX + 1.5, BASE, 15, 1.5, dx, dy);

    for (let r = 0; r < 6; r++) for (let c = 0; c < 6; c++) {
      chk(`${tag} cell`, RX + c * 3 + 1, RY + r * 3, 3, 3, dx, dy); // +1 = jolt
    }
    for (const [x, y, w, h] of [[7.5,0,1.5,4.5],[9,4.5,1.5,3],[7.5,7.5,1.5,3],
        [9,10.5,1.5,4.5],[3,6,4.5,1.5],[10.5,9,4.5,1.5],[4.5,13.5,1.5,4.5]]) {
      chk(`${tag} crack`, RX + x + 1, RY + y, w, h, dx, dy);
    }
    chk(`${tag} pop`, RX + 4, RY + 6, 12, 12, dx, dy);

    // Clawd
    [9,15,30,36].forEach(x => chk(`${tag} leg`, x + OX, 39 + OY, 3, 9, dx, dy));
    chk(`${tag} armL`, 0 + OX, 33 + OY + bob, 7.5, 6, dx, dy);
    chk(`${tag} armR`, 40.5 + OX, 33 + OY + bob + p.armDY, 7.5, 6, dx, dy);
    chk(`${tag} body`, 6 + OX, 18 + OY + bob, 36, 24, dx, dy);
    // The mine's headgear, which now lives in ui.html's HAT table. No halo:
    // that rect was checked here for a long while after the page stopped
    // drawing it, which is the drift assertMirrorsUi exists to prevent.
    chk(`${tag} brim`, 6 + DX, 14 + DY + bob, 44, 3, dx, dy);
    chk(`${tag} dome`, 12 + DX, 8 + DY + bob, 32, 6, dx, dy);
    chk(`${tag} lampbx`, 23 + DX, 9 + DY + bob, 10, 5, dx, dy);
    chk(`${tag} lamp`, 25 + DX, 10 + DY + bob, 6, 3, dx, dy);
    chk(`${tag} eye`, 12 + OX + 1, 24 + OY + bob, 3, 6, dx, dy);

    // Pickaxe: overlapping block line for the shaft, plus the perpendicular
    // head bar. Both are snapped to the half-unit grid.
    const snap = v => Math.round(v * 2) / 2;
    const gaps = [];
    function line(label, x0, y0, x1, y1, steps, from) {
      let prev = null;
      for (let i = from; i <= steps; i++) {
        const t = i / steps;
        const bx = snap(x0 + (x1 - x0) * t) - 1.5;
        const by = snap(y0 + (y1 - y0) * t) - 1.5;
        chk(`${tag} ${label}${i}`, bx, by, 3, 3, dx, dy);
        // Consecutive 3-unit blocks must actually overlap, otherwise the shaft
        // renders as a row of disconnected squares.
        if (prev && (Math.abs(bx - prev[0]) >= 3 || Math.abs(by - prev[1]) >= 3)) {
          gaps.push(`${tag} ${label}: ${prev} -> ${[bx, by]}`);
        }
        prev = [bx, by];
      }
    }
    const hx = 52 + DX, hy = 35 + DY + bob + p.armDY;
    const [sx, sy] = p.step;
    const cx = hx + sx * 4, cy = hy + sy * 4;
    line('handle', hx, hy, cx, cy, 8, 1);
    const len = Math.hypot(sx, sy);
    const px = -sy / len * 3, py = sx / len * 3;
    line('head', cx - px, cy - py, cx + px, cy + py, 4, 0);
    chk(`${tag} tip`, snap(cx + px) - 1.5, snap(cy + py) - 1.5, 3, 1.5, dx, dy);
    problems.push(...gaps);
  }

  // Amount row, from the narrowest to a six-figure total. Glyph boxes are
  // checked individually because the dollar sign is two rows taller than the
  // digits and hangs outside the nominal text box.
  //
  // The long strings are not hypothetical: the counter can be set to a rolling
  // 30-day window, and a month at this machine's rate is already five figures.
  for (const text of ['$--.--', '$0.00', '$33.47', '$123.45', '$1234.56',
                      '$9999.99', '$12345.67', '$123456.78']) {
    const scale = amountScale(text), width = textWidth(text, scale);
    const y = amountY(scale);
    let cx = Math.floor((W - width) / 2);
    for (const ch of text) {
      const rows = FONT[ch];
      const gy = (5 - rows.length) / 2 * scale;
      chk(`d(${dx},${dy}) glyph '${ch}' of "${text}"`,
          cx, y + gy, rows[0].length * scale, rows.length * scale, dx, dy);
      cx += (rows[0].length + 1) * scale;
    }
    const x = Math.floor((W - width) / 2);
    chk(`d(${dx},${dy}) stale-dot "${text}"`, x + width + 3, y, 2, 2, dx, dy);
  }
}

// The step down must happen exactly where the row would otherwise overflow, and
// nowhere earlier — a four-figure day shrinking would be a visible regression.
for (const [text, want] of [['$1234.56', 3], ['$9999.99', 3], ['$12345.67', 2],
                            ['$123456.78', 2], ['$--.--', 3]]) {
  const got = amountScale(text);
  if (got !== want) problems.push(`scale for "${text}": expected ${want}, got ${got}`);
}

console.log(`content bounds x ${minX} -> ${maxX}   y ${minY} -> ${maxY}   stage ${W}x${H}`);
console.log(`amount widths: ` + ['$--.--','$33.47','$1234.56','$12345.67','$123456.78']
  .map(t => `${t}=${textWidth(t, amountScale(t))}@x${amountScale(t)}`).join('  '));
console.log(problems.length ? `PROBLEMS (${problems.length}):\n` + problems.slice(0, 25).join('\n')
                            : 'ALL RECTS PIXEL-ALIGNED AND IN BOUNDS (4 poses x 5 drift offsets)');
// Without this the one check that predates the others is the one that cannot
// fail a script: it printed its complaints and exited 0 like everything was well.
process.exit(problems.length ? 1 : 0);
