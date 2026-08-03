// Geometry check for saver/src/ui.html: every rect must land on whole device
// pixels and stay inside the stage, for every pose, every drift offset and the
// widest amount string we expect to render.
const W = 112, H = 74, DX = 13, DY = 19;
const OX = 4 + DX, OY = -1 + DY, BASE = 47 + DY;
const RX = 64 + DX, RY = 29 + DY;
const S = 14; // even, as resize() guarantees

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
  '-': ['000','000','111','000','000'], '?': ['111','001','011','000','010'],
};

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
    chk(`${tag} brim`, 6 + DX, 14 + DY + bob, 44, 3, dx, dy);
    chk(`${tag} dome`, 12 + DX, 8 + DY + bob, 32, 6, dx, dy);
    chk(`${tag} glow`, 21 + DX, 7 + DY + bob, 14, 9, dx, dy);
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

  // Amount row, from the narrowest to a wide four-figure day. Glyph boxes are
  // checked individually because the dollar sign is two rows taller than the
  // digits and hangs outside the nominal text box.
  for (const text of ['$--.--', '$0.00', '$33.47', '$123.45', '$1234.56']) {
    const scale = 3, width = textWidth(text, scale);
    let cx = Math.floor((W - width) / 2);
    for (const ch of text) {
      const rows = FONT[ch];
      const gy = (5 - rows.length) / 2 * scale;
      chk(`d(${dx},${dy}) glyph '${ch}' of "${text}"`,
          cx, 6 + gy, rows[0].length * scale, rows.length * scale, dx, dy);
      cx += (rows[0].length + 1) * scale;
    }
    const x = Math.floor((W - width) / 2);
    chk(`d(${dx},${dy}) stale-dot "${text}"`, x + width + 3, 6, 2, 2, dx, dy);
  }
}

console.log(`content bounds x ${minX} -> ${maxX}   y ${minY} -> ${maxY}   stage ${W}x${H}`);
console.log(`amount widths: ` + ['$--.--','$33.47','$123.45','$1234.56']
  .map(t => `${t}=${textWidth(t, 3)}`).join('  '));
console.log(problems.length ? `PROBLEMS (${problems.length}):\n` + problems.slice(0, 25).join('\n')
                            : 'ALL RECTS PIXEL-ALIGNED AND IN BOUNDS (4 poses x 5 drift offsets)');
