'use strict';
/* ===== Utilitaires ===== */
const $ = (s, r = document) => r.querySelector(s);
const uid = () => Math.random().toString(36).slice(2, 9) + Date.now().toString(36).slice(-4);
const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
const r2 = n => Math.round(n * 100) / 100;

/* ===== Constantes ===== */
const FONTS = {
  sans: '"Segoe UI", system-ui, -apple-system, Roboto, Helvetica, Arial, sans-serif',
  hand: '"Segoe Print", "Bradley Hand", "Comic Sans MS", "Chalkboard SE", cursive',
  serif: 'Georgia, "Times New Roman", serif',
  mono: 'Consolas, "SF Mono", "Courier New", monospace',
};
const BACKGROUNDS = [
  { id: 'white', name: 'Blanc', c: '#ffffff', dark: false },
  { id: 'cream', name: 'Crème', c: '#fbf5e6', dark: false },
  { id: 'gray', name: 'Gris', c: '#e9ecef', dark: false },
  { id: 'slate', name: 'Ardoise', c: '#262b38', dark: true },
  { id: 'black', name: 'Noir', c: '#101114', dark: true },
  { id: 'green', name: 'Tableau vert', c: '#1e3a31', dark: true },
];
const PATTERNS = [['none', 'Aucun'], ['dots', 'Points'], ['grid', 'Quadrillage'], ['lines', 'Lignes']];
const GRID = 24;
const COLORS = ['ink', '#e03131', '#f08c00', '#f2c600', '#2f9e44', '#0c8599', '#1971c2', '#7048e8', '#d6336c', '#868e96'];
const FILLS = ['transparent', '#ffc9c9', '#ffd8a8', '#ffec99', '#b2f2bb', '#99e9f2', '#a5d8ff', '#d0bfff', '#fcc2d7', '#dee2e6', '#ffffff'];
const NOTES = ['#ffec99', '#ffd8a8', '#ffc9c9', '#fcc2d7', '#d0bfff', '#a5d8ff', '#99e9f2', '#b2f2bb', '#dee2e6'];
const SHAPES = ['rect', 'ellipse', 'diamond', 'triangle'];
const LABELED = [...SHAPES, 'sticky', 'text'];
const ACCENT = '#5b7cfa';

const TOOLS = [
  { id: 'select', key: 'v', label: 'Sélection', icon: 'select' },
  { id: 'hand', key: 'h', label: 'Main (déplacer la vue)', icon: 'hand' },
  { sep: 1 },
  { id: 'pen', key: 'p', label: 'Crayon', icon: 'pen' },
  { id: 'highlighter', key: 'm', label: 'Surligneur', icon: 'highlighter' },
  { id: 'eraser', key: 'e', label: 'Gomme', icon: 'eraser' },
  { sep: 1 },
  { id: 'rect', key: 'r', label: 'Rectangle', icon: 'rect' },
  { id: 'ellipse', key: 'o', label: 'Ellipse', icon: 'ellipse' },
  { id: 'diamond', key: 'd', label: 'Losange', icon: 'diamond' },
  { id: 'triangle', key: 'y', label: 'Triangle', icon: 'triangle' },
  { id: 'line', key: 'l', label: 'Ligne', icon: 'line' },
  { id: 'arrow', key: 'a', label: 'Flèche', icon: 'arrow' },
  { sep: 1 },
  { id: 'text', key: 't', label: 'Texte', icon: 'text' },
  { id: 'sticky', key: 'n', label: 'Note adhésive', icon: 'sticky' },
  { id: 'image', key: 'i', label: 'Image', icon: 'image' },
  { id: 'laser', key: 'f', label: 'Pointeur laser', icon: 'laser' },
];

/* ===== État global ===== */
const S = {
  id: null, name: 'Sans titre',
  objects: [], assets: {}, sel: new Set(),
  cam: { x: 0, y: 0, z: 1 }, bg: 'white', pattern: 'dots', snap: false,
  tool: 'select', toolLock: false,
  style: { stroke: 'ink', fill: 'transparent', noteFill: '#ffec99', sw: 3, dash: 'solid', opacity: 1, round: true, font: 'sans', fontSize: 24, align: 'left', heads: 'end' },
  penStyle: null, hlStyle: { stroke: '#f2c600', sw: 18, opacity: 0.4 },
  draft: null, marquee: null, hover: null, erasing: null, laser: [], editing: null,
};

const bgInfo = () => BACKGROUNDS.find(b => b.id === S.bg) || BACKGROUNDS[0];
const inkColor = () => (bgInfo().dark ? '#f1f3f5' : '#1e1e1e');
const col = c => (c === 'ink' ? inkColor() : c);
const byId = id => S.objects.find(o => o.id === id);
const selObjs = () => S.objects.filter(o => S.sel.has(o.id));

/* ===== Canvas & caméra ===== */
const cv = $('#cv');
const ctx = cv.getContext('2d');
const mctx = document.createElement('canvas').getContext('2d');
let VW = 0, VH = 0, DPR = 1, rafId = 0;

function resizeCanvas() {
  DPR = window.devicePixelRatio || 1;
  VW = window.innerWidth; VH = window.innerHeight;
  cv.width = Math.round(VW * DPR); cv.height = Math.round(VH * DPR);
  cv.style.width = VW + 'px'; cv.style.height = VH + 'px';
  requestRender();
}
function requestRender() {
  if (!rafId) rafId = requestAnimationFrame(() => { rafId = 0; render(); });
}
const s2w = (x, y) => ({ x: (x - S.cam.x) / S.cam.z, y: (y - S.cam.y) / S.cam.z });
const w2s = (x, y) => ({ x: x * S.cam.z + S.cam.x, y: y * S.cam.z + S.cam.y });

/* ===== Géométrie ===== */
function toLocal(o, wx, wy) {
  const dx = wx - (o.x + o.w / 2), dy = wy - (o.y + o.h / 2);
  const a = -(o.angle || 0), c = Math.cos(a), s = Math.sin(a);
  return { x: dx * c - dy * s, y: dx * s + dy * c };
}
function toWorld(o, lx, ly) {
  const a = o.angle || 0, c = Math.cos(a), s = Math.sin(a);
  return { x: o.x + o.w / 2 + lx * c - ly * s, y: o.y + o.h / 2 + lx * s + ly * c };
}
function bboxOf(o) {
  if (!o.angle) return { x: o.x, y: o.y, w: o.w, h: o.h };
  const hw = o.w / 2, hh = o.h / 2;
  const ps = [[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]].map(p => toWorld(o, p[0], p[1]));
  const xs = ps.map(p => p.x), ys = ps.map(p => p.y);
  const x = Math.min(...xs), y = Math.min(...ys);
  return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
}
function unionBox(list) {
  if (!list.length) return null;
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const o of list) {
    const b = bboxOf(o);
    x0 = Math.min(x0, b.x); y0 = Math.min(y0, b.y);
    x1 = Math.max(x1, b.x + b.w); y1 = Math.max(y1, b.y + b.h);
  }
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
}
function segDist(px, py, ax, ay, bx, by) {
  const dx = bx - ax, dy = by - ay, l2 = dx * dx + dy * dy;
  let t = l2 ? ((px - ax) * dx + (py - ay) * dy) / l2 : 0;
  t = clamp(t, 0, 1);
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}
function inPoly(poly, x, y) {
  let c = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i], [xj, yj] = poly[j];
    if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c;
  }
  return c;
}
function polyOf(o) {
  const hw = o.w / 2, hh = o.h / 2;
  if (o.type === 'diamond') return [[0, -hh], [hw, 0], [0, hh], [-hw, 0]];
  if (o.type === 'triangle') return [[0, -hh], [hw, hh], [-hw, hh]];
  return [[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]];
}
const localPts = o => o.pts.map(p => [(p[0] - 0.5) * o.w, (p[1] - 0.5) * o.h]);
const absPts = o => o.pts.map(p => { const w = toWorld(o, (p[0] - 0.5) * o.w, (p[1] - 0.5) * o.h); return [w.x, w.y]; });

/** (Re)calcule x,y,w,h,pts à partir de points absolus (pen / line). */
function setAbsPoints(o, pts) {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const p of pts) { x0 = Math.min(x0, p[0]); y0 = Math.min(y0, p[1]); x1 = Math.max(x1, p[0]); y1 = Math.max(y1, p[1]); }
  const w = x1 - x0, h = y1 - y0;
  o.x = x0; o.y = y0; o.w = w; o.h = h; o.angle = 0;
  o.pts = pts.map(p => [r2((p[0] - x0) / (w || 1)), r2((p[1] - y0) / (h || 1))]);
  return o;
}
/** Applique une rotation autour d'un point ; les lignes sont "cuites" dans leurs points. */
function rotateAround(o, cx, cy, da) {
  if (o.type === 'line') {
    const c = Math.cos(da), s = Math.sin(da);
    setAbsPoints(o, absPts(o).map(([x, y]) => [cx + (x - cx) * c - (y - cy) * s, cy + (x - cx) * s + (y - cy) * c]));
    return;
  }
  const mx = o.x + o.w / 2, my = o.y + o.h / 2, c = Math.cos(da), s = Math.sin(da);
  const nx = cx + (mx - cx) * c - (my - cy) * s, ny = cy + (mx - cx) * s + (my - cy) * c;
  o.x = nx - o.w / 2; o.y = ny - o.h / 2; o.angle = (o.angle || 0) + da;
}
function rdp(pts, eps) {
  if (pts.length < 3) return pts;
  const keep = new Uint8Array(pts.length); keep[0] = keep[pts.length - 1] = 1;
  const stack = [[0, pts.length - 1]];
  while (stack.length) {
    const [a, b] = stack.pop(); let md = 0, mi = -1;
    for (let i = a + 1; i < b; i++) {
      const d = segDist(pts[i][0], pts[i][1], pts[a][0], pts[a][1], pts[b][0], pts[b][1]);
      if (d > md) { md = d; mi = i; }
    }
    if (md > eps && mi > 0) { keep[mi] = 1; stack.push([a, mi], [mi, b]); }
  }
  return pts.filter((_, i) => keep[i]);
}

function hitObj(o, wx, wy, tol) {
  const p = toLocal(o, wx, wy), hw = o.w / 2, hh = o.h / 2, t = tol + (o.sw || 0) / 2;
  switch (o.type) {
    case 'pen': case 'line': {
      const pts = localPts(o);
      for (let i = 0; i < pts.length - 1; i++) if (segDist(p.x, p.y, pts[i][0], pts[i][1], pts[i + 1][0], pts[i + 1][1]) <= t) return true;
      return pts.length === 1 && Math.hypot(p.x - pts[0][0], p.y - pts[0][1]) <= t;
    }
    case 'text': case 'image': case 'sticky':
      return Math.abs(p.x) <= hw + tol && Math.abs(p.y) <= hh + tol;
    case 'ellipse': {
      const inside = (rx, ry) => rx > 0 && ry > 0 && (p.x / rx) ** 2 + (p.y / ry) ** 2 <= 1;
      if (inside(hw + t, hh + t) && (o.fill !== 'transparent' || o.text)) return true;
      return inside(hw + t, hh + t) && !inside(hw - t, hh - t);
    }
    default: {
      const poly = polyOf(o);
      if ((o.fill !== 'transparent' || o.text) && inPoly(poly, p.x, p.y)) return true;
      for (let i = 0; i < poly.length; i++) {
        const a = poly[i], b = poly[(i + 1) % poly.length];
        if (segDist(p.x, p.y, a[0], a[1], b[0], b[1]) <= t) return true;
      }
      return false;
    }
  }
}
function pickAt(wx, wy, tol) {
  for (let i = S.objects.length - 1; i >= 0; i--) if (hitObj(S.objects[i], wx, wy, tol)) return S.objects[i];
  return null;
}
function expandGroups(ids) {
  const g = new Set();
  for (const o of S.objects) if (ids.has(o.id) && o.gid) g.add(o.gid);
  if (!g.size) return ids;
  const out = new Set(ids);
  for (const o of S.objects) if (o.gid && g.has(o.gid)) out.add(o.id);
  return out;
}

/* ===== Texte ===== */
const lineHeight = o => o.fontSize * 1.3;
const fontStr = (o, z = 1) => `${o.fontSize * z}px ${FONTS[o.font] || FONTS.sans}`;

function breakWord(word, maxW) {
  const parts = []; let cur = '';
  for (const ch of word) {
    if (mctx.measureText(cur + ch).width > maxW && cur) { parts.push(cur); cur = ch; } else cur += ch;
  }
  parts.push(cur); return parts;
}
function wrapLines(text, maxW) {
  const out = [];
  for (const para of String(text).split('\n')) {
    if (!para) { out.push(''); continue; }
    let line = '';
    for (const word of para.split(' ')) {
      const test = line ? line + ' ' + word : word;
      if (mctx.measureText(test).width <= maxW || !line) {
        if (mctx.measureText(test).width > maxW) {
          const parts = breakWord(word, maxW);
          if (line) { out.push(line); line = ''; }
          for (let i = 0; i < parts.length - 1; i++) out.push(parts[i]);
          line = parts[parts.length - 1];
        } else line = test;
      } else {
        out.push(line);
        if (mctx.measureText(word).width > maxW) {
          const parts = breakWord(word, maxW);
          for (let i = 0; i < parts.length - 1; i++) out.push(parts[i]);
          line = parts[parts.length - 1];
        } else line = word;
      }
    }
    out.push(line);
  }
  return out;
}
function labelBox(o) {
  const hw = o.w / 2, hh = o.h / 2;
  switch (o.type) {
    case 'text': return { x: -hw + 3, y: -hh, w: Math.max(1, o.w - 6), h: o.h };
    case 'sticky': return { x: -hw + 14, y: -hh + 14, w: Math.max(1, o.w - 28), h: Math.max(1, o.h - 28) };
    case 'ellipse': return { x: -hw * 0.7, y: -hh * 0.7, w: o.w * 0.7, h: o.h * 0.7 };
    case 'diamond': return { x: -hw * 0.5, y: -hh * 0.5, w: o.w * 0.5, h: o.h * 0.5 };
    case 'triangle': return { x: -hw * 0.5, y: -hh * 0.1, w: o.w * 0.5, h: o.h * 0.5 };
    default: return { x: -hw + 8, y: -hh + 8, w: Math.max(1, o.w - 16), h: Math.max(1, o.h - 16) };
  }
}
function labelInfo(o) {
  const box = labelBox(o), lh = lineHeight(o);
  mctx.font = fontStr(o);
  const lines = o.type === 'text' ? String(o.text || '').split('\n') : wrapLines(o.text || '', box.w);
  return { box, lines, lh, total: lines.length * lh };
}
function fitText(o) {
  mctx.font = fontStr(o);
  const lines = String(o.text || '').split('\n'); let w = 0;
  for (const l of lines) w = Math.max(w, mctx.measureText(l).width);
  o.w = Math.max(12, Math.ceil(w) + 6); o.h = Math.max(1, lines.length) * lineHeight(o);
}
function ensureFit(o) {
  if (o.type === 'text') return fitText(o);
  if (!o.text) return;
  const li = labelInfo(o);
  if (li.total > li.box.h) {
    const ratio = { ellipse: 0.7, diamond: 0.5, triangle: 0.5 }[o.type] || 1;
    o.h += (li.total - li.box.h) / ratio;
  }
}

/* ===== Images ===== */
const IMG = new Map();
function getImage(id) {
  let im = IMG.get(id);
  if (!im && S.assets[id]) { im = new Image(); im.onload = requestRender; im.src = S.assets[id]; IMG.set(id, im); }
  return im;
}

/* ===== Dessin des objets ===== */
function penD(pts) {
  const f = n => r2(n);
  let d = `M${f(pts[0][0])} ${f(pts[0][1])}`;
  if (pts.length === 1) return d + `L${f(pts[0][0])} ${f(pts[0][1])}`;
  for (let i = 1; i < pts.length - 1; i++) {
    d += `Q${f(pts[i][0])} ${f(pts[i][1])} ${f((pts[i][0] + pts[i + 1][0]) / 2)} ${f((pts[i][1] + pts[i + 1][1]) / 2)}`;
  }
  const l = pts[pts.length - 1];
  return d + `L${f(l[0])} ${f(l[1])}`;
}
function getPath(o) {
  if (o._pp !== o.pts || o._pw !== o.w || o._ph !== o.h) {
    o._path = new Path2D(penD(localPts(o))); o._pp = o.pts; o._pw = o.w; o._ph = o.h;
  }
  return o._path;
}
function dashArr(o) {
  if (o.dash === 'dashed') return [o.sw * 3.2 + 3, o.sw * 2.4 + 3];
  if (o.dash === 'dotted') return [0.1, o.sw * 2.2 + 2];
  return [];
}
function headPts(ex, ey, px, py, sw) {
  const a = Math.atan2(ey - py, ex - px), L = Math.max(12, sw * 4.5), s = 0.5;
  return [[ex - L * Math.cos(a - s), ey - L * Math.sin(a - s)], [ex, ey], [ex - L * Math.cos(a + s), ey - L * Math.sin(a + s)]];
}
function polyPath(c, poly) {
  c.beginPath(); poly.forEach((p, i) => (i ? c.lineTo(p[0], p[1]) : c.moveTo(p[0], p[1]))); c.closePath();
}

function drawObject(c, o, skipLabel) {
  c.save();
  c.globalAlpha = (o.opacity ?? 1) * (o._ghost ? 0.3 : 1);
  c.translate(o.x + o.w / 2, o.y + o.h / 2);
  if (o.angle) c.rotate(o.angle);
  const hw = o.w / 2, hh = o.h / 2;
  c.lineCap = 'round'; c.lineJoin = 'round';
  c.lineWidth = o.sw; c.strokeStyle = col(o.stroke); c.setLineDash(dashArr(o));
  switch (o.type) {
    case 'pen': {
      if (o.hl) c.lineCap = 'butt';
      c.stroke(getPath(o)); break;
    }
    case 'line': {
      const p = localPts(o);
      c.beginPath(); c.moveTo(p[0][0], p[0][1]); c.lineTo(p[1][0], p[1][1]); c.stroke();
      c.setLineDash([]);
      for (const [pi, qi, on] of [[1, 0, o.arrowEnd], [0, 1, o.arrowStart]]) {
        if (!on) continue;
        const h = headPts(p[pi][0], p[pi][1], p[qi][0], p[qi][1], o.sw);
        c.beginPath(); c.moveTo(h[0][0], h[0][1]); c.lineTo(h[1][0], h[1][1]); c.lineTo(h[2][0], h[2][1]); c.stroke();
      }
      break;
    }
    case 'rect': case 'ellipse': case 'diamond': case 'triangle': {
      c.beginPath();
      if (o.type === 'rect') {
        const r = o.round ? Math.min(28, Math.min(o.w, o.h) * 0.22) : 0;
        if (r && c.roundRect) c.roundRect(-hw, -hh, o.w, o.h, r); else c.rect(-hw, -hh, o.w, o.h);
      } else if (o.type === 'ellipse') c.ellipse(0, 0, Math.max(hw, 0.01), Math.max(hh, 0.01), 0, 0, Math.PI * 2);
      else polyPath(c, polyOf(o));
      if (o.fill !== 'transparent') { c.fillStyle = col(o.fill); c.fill(); }
      if (o.sw > 0) c.stroke();
      if (!skipLabel) drawLabel(c, o, col(o.stroke));
      break;
    }
    case 'sticky': {
      c.setLineDash([]);
      c.fillStyle = 'rgba(0,0,0,.10)'; c.fillRect(-hw + 4, -hh + 8, o.w, o.h);
      c.fillStyle = 'rgba(0,0,0,.08)'; c.fillRect(-hw + 2, -hh + 4, o.w, o.h);
      c.fillStyle = o.fill; c.fillRect(-hw, -hh, o.w, o.h);
      if (!skipLabel) drawLabel(c, o, '#1e1e1e');
      break;
    }
    case 'text':
      if (!skipLabel) drawLabel(c, o, col(o.stroke));
      break;
    case 'image': {
      const im = getImage(o.asset);
      if (im && im.complete && im.naturalWidth) c.drawImage(im, -hw, -hh, o.w, o.h);
      else { c.fillStyle = 'rgba(128,128,128,.25)'; c.fillRect(-hw, -hh, o.w, o.h); }
      break;
    }
  }
  c.restore();
}
function drawLabel(c, o, color) {
  if (!o.text) return;
  const li = labelInfo(o), b = li.box;
  c.setLineDash([]); c.font = fontStr(o); c.fillStyle = color; c.textBaseline = 'middle';
  c.textAlign = o.align === 'center' ? 'center' : o.align === 'right' ? 'right' : 'left';
  const x = o.align === 'center' ? b.x + b.w / 2 : o.align === 'right' ? b.x + b.w : b.x;
  let y = b.y + (b.h - li.total) / 2 + li.lh / 2;
  if (li.total > b.h) y = b.y + li.lh / 2;
  for (const l of li.lines) { c.fillText(l, x, y); y += li.lh; }
}

/* ===== Poignées de sélection ===== */
function selectionBox() {
  const objs = selObjs();
  if (!objs.length) return null;
  const locked = objs.every(o => o.locked);
  if (objs.length === 1) {
    const o = objs[0];
    if (o.type === 'line') return { kind: 'line', o, locked };
    return { kind: 'single', o, locked, box: { x: o.x, y: o.y, w: o.w, h: o.h, angle: o.angle || 0 } };
  }
  const b = unionBox(objs);
  return { kind: 'multi', objs, locked, box: { x: b.x, y: b.y, w: b.w, h: b.h, angle: 0 } };
}
function getHandles() {
  const sb = selectionBox();
  if (!sb || sb.locked || S.editing) return [];
  if (sb.kind === 'line') {
    const p = absPts(sb.o);
    return [{ id: 'p0', x: p[0][0], y: p[0][1] }, { id: 'p1', x: p[1][0], y: p[1][1] }];
  }
  const b = sb.box, hw = b.w / 2, hh = b.h / 2;
  const text = sb.kind === 'single' && sb.o.type === 'text';
  const defs = [['nw', -hw, -hh], ['ne', hw, -hh], ['se', hw, hh], ['sw', -hw, hh]];
  if (!text) defs.push(['n', 0, -hh], ['e', hw, 0], ['s', 0, hh], ['w', -hw, 0]);
  defs.push(['rot', 0, -hh - 28 / S.cam.z]);
  return defs.map(([id, lx, ly]) => { const w = toWorld(b, lx, ly); return { id, x: w.x, y: w.y }; });
}
function hitHandle(wx, wy) {
  const hs = getHandles(); let best = null, bd = 10;
  for (const h of hs) {
    const d = Math.hypot(h.x - wx, h.y - wy) * S.cam.z;
    if (d <= bd) { bd = d; best = h; }
  }
  return best;
}

/* ===== Rendu principal ===== */
function drawBackground() {
  const bg = bgInfo();
  ctx.fillStyle = bg.c; ctx.fillRect(0, 0, VW, VH);
  if (S.pattern === 'none') return;
  let step = GRID * S.cam.z;
  while (step < 14) step *= 2;
  const ox = ((S.cam.x % step) + step) % step, oy = ((S.cam.y % step) + step) % step;
  const base = bg.dark ? '255,255,255' : '0,0,0';
  if (S.pattern === 'dots') {
    ctx.fillStyle = `rgba(${base},.2)`;
    for (let x = ox; x < VW; x += step) for (let y = oy; y < VH; y += step) ctx.fillRect(x - 1, y - 1, 2, 2);
  } else {
    ctx.strokeStyle = `rgba(${base},.09)`; ctx.lineWidth = 1; ctx.beginPath();
    if (S.pattern === 'grid') for (let x = ox; x < VW; x += step) { ctx.moveTo(Math.round(x) + .5, 0); ctx.lineTo(Math.round(x) + .5, VH); }
    for (let y = oy; y < VH; y += step) { ctx.moveTo(0, Math.round(y) + .5); ctx.lineTo(VW, Math.round(y) + .5); }
    ctx.stroke();
  }
}
function render() {
  ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
  drawBackground();
  ctx.save();
  ctx.translate(S.cam.x, S.cam.y); ctx.scale(S.cam.z, S.cam.z);
  const v0 = s2w(0, 0), v1 = s2w(VW, VH);
  for (const o of S.objects) {
    const b = bboxOf(o), m = (o.sw || 0) + 30;
    if (b.x > v1.x + m || b.y > v1.y + m || b.x + b.w < v0.x - m || b.y + b.h < v0.y - m) continue;
    drawObject(ctx, o, S.editing === o.id);
  }
  if (S.draft) drawObject(ctx, S.draft);
  drawOverlay();
  ctx.restore();
  drawLaser();
}
function drawOverlay() {
  const z = S.cam.z;
  ctx.lineWidth = 1.5 / z; ctx.strokeStyle = ACCENT; ctx.setLineDash([]);
  if (S.hover && !S.sel.has(S.hover)) {
    const o = byId(S.hover);
    if (o && o.type !== 'line') outline(o, 1 / z, 0.55);
  }
  const objs = selObjs();
  for (const o of objs) {
    if (o.type === 'line') continue;
    outline(o, 1.5 / z, 1);
  }
  const sb = selectionBox();
  if (sb && sb.kind === 'multi') {
    ctx.save(); ctx.setLineDash([5 / z, 4 / z]); ctx.lineWidth = 1 / z;
    const b = sb.box; ctx.strokeRect(b.x, b.y, b.w, b.h); ctx.restore();
  }
  for (const h of getHandles()) {
    ctx.beginPath();
    ctx.fillStyle = '#fff'; ctx.strokeStyle = ACCENT; ctx.lineWidth = 1.5 / z;
    if (h.id === 'rot' || h.id[0] === 'p') ctx.arc(h.x, h.y, 5.5 / z, 0, Math.PI * 2);
    else ctx.rect(h.x - 4.5 / z, h.y - 4.5 / z, 9 / z, 9 / z);
    ctx.fill(); ctx.stroke();
  }
  if (S.marquee) {
    const m = S.marquee;
    ctx.fillStyle = 'rgba(91,124,250,.10)'; ctx.strokeStyle = ACCENT; ctx.lineWidth = 1 / z;
    ctx.fillRect(m.x0, m.y0, m.x1 - m.x0, m.y1 - m.y0); ctx.strokeRect(m.x0, m.y0, m.x1 - m.x0, m.y1 - m.y0);
  }
}
function outline(o, lw, alpha) {
  ctx.save();
  ctx.globalAlpha = alpha; ctx.lineWidth = lw; ctx.strokeStyle = ACCENT;
  ctx.translate(o.x + o.w / 2, o.y + o.h / 2); if (o.angle) ctx.rotate(o.angle);
  ctx.strokeRect(-o.w / 2, -o.h / 2, o.w, o.h);
  ctx.restore();
}
function drawLaser() {
  const now = performance.now();
  S.laser = S.laser.filter(p => now - p.t < 900);
  if (!S.laser.length) return;
  ctx.save(); ctx.lineCap = 'round'; ctx.lineJoin = 'round';
  for (let i = 1; i < S.laser.length; i++) {
    const a = S.laser[i - 1], b = S.laser[i];
    if (b.brk) continue;
    const k = 1 - (now - b.t) / 900;
    const pa = w2s(a.x, a.y), pb = w2s(b.x, b.y);
    ctx.strokeStyle = `rgba(255,40,40,${0.25 * k})`; ctx.lineWidth = 12 * k + 2;
    ctx.beginPath(); ctx.moveTo(pa.x, pa.y); ctx.lineTo(pb.x, pb.y); ctx.stroke();
    ctx.strokeStyle = `rgba(255,70,70,${k})`; ctx.lineWidth = 4 * k + 1;
    ctx.beginPath(); ctx.moveTo(pa.x, pa.y); ctx.lineTo(pb.x, pb.y); ctx.stroke();
  }
  const l = S.laser[S.laser.length - 1];
  if (now - l.t < 120) {
    const p = w2s(l.x, l.y);
    ctx.fillStyle = 'rgba(255,40,40,.95)'; ctx.beginPath(); ctx.arc(p.x, p.y, 6, 0, Math.PI * 2); ctx.fill();
  }
  ctx.restore();
  requestRender();
}

/* ===== Exports ===== */
function exportBounds(objs, pad) {
  const b = unionBox(objs); if (!b) return null;
  let m = 0; for (const o of objs) m = Math.max(m, (o.sw || 0) / 2);
  const p = pad + m + (objs.some(o => o.type === 'sticky') ? 10 : 0);
  return { x: b.x - p, y: b.y - p, w: b.w + 2 * p, h: b.h + 2 * p };
}
function exportCanvas(objs, { scale = 2, pad = 24, transparent = false, maxDim = 12000 } = {}) {
  const b = exportBounds(objs, pad); if (!b) return null;
  scale = Math.min(scale, maxDim / Math.max(b.w, b.h));
  const c = document.createElement('canvas');
  c.width = Math.max(1, Math.ceil(b.w * scale)); c.height = Math.max(1, Math.ceil(b.h * scale));
  const x = c.getContext('2d');
  if (!transparent) { x.fillStyle = bgInfo().c; x.fillRect(0, 0, c.width, c.height); }
  x.scale(scale, scale); x.translate(-b.x, -b.y);
  for (const o of objs) drawObject(x, o);
  return c;
}
const esc = s => String(s).replace(/[&<>"]/g, ch => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[ch]));
function svgDash(o) { const d = dashArr(o); return d.length ? ` stroke-dasharray="${d.join(' ')}"` : ''; }
function svgLabel(o, color) {
  if (!o.text) return '';
  const li = labelInfo(o), b = li.box;
  const anchor = o.align === 'center' ? 'middle' : o.align === 'right' ? 'end' : 'start';
  const x = o.align === 'center' ? b.x + b.w / 2 : o.align === 'right' ? b.x + b.w : b.x;
  let y = li.total > b.h ? b.y + li.lh / 2 : b.y + (b.h - li.total) / 2 + li.lh / 2, out = '';
  for (const l of li.lines) {
    out += `<text x="${r2(x)}" y="${r2(y)}" text-anchor="${anchor}" dominant-baseline="central" xml:space="preserve">${esc(l)}</text>`;
    y += li.lh;
  }
  return `<g font-family="${esc(FONTS[o.font] || FONTS.sans).replace(/&quot;/g, "'")}" font-size="${o.fontSize}" fill="${color}">${out}</g>`;
}
function objToSVG(o) {
  const hw = o.w / 2, hh = o.h / 2;
  const tf = `translate(${r2(o.x + hw)} ${r2(o.y + hh)})${o.angle ? ` rotate(${r2((o.angle * 180) / Math.PI)})` : ''}`;
  const op = (o.opacity ?? 1) < 1 ? ` opacity="${o.opacity}"` : '';
  const st = `stroke="${col(o.stroke)}" stroke-width="${o.sw}" stroke-linecap="${o.hl ? 'butt' : 'round'}" stroke-linejoin="round"${svgDash(o)}`;
  const fill = o.fill && o.fill !== 'transparent' ? col(o.fill) : 'none';
  let inner = '';
  switch (o.type) {
    case 'pen': inner = `<path d="${penD(localPts(o))}" fill="none" ${st}/>`; break;
    case 'line': {
      const p = localPts(o);
      inner = `<path d="M${r2(p[0][0])} ${r2(p[0][1])}L${r2(p[1][0])} ${r2(p[1][1])}" fill="none" ${st}/>`;
      for (const [pi, qi, on] of [[1, 0, o.arrowEnd], [0, 1, o.arrowStart]]) {
        if (!on) continue;
        const h = headPts(p[pi][0], p[pi][1], p[qi][0], p[qi][1], o.sw);
        inner += `<path d="M${r2(h[0][0])} ${r2(h[0][1])}L${r2(h[1][0])} ${r2(h[1][1])}L${r2(h[2][0])} ${r2(h[2][1])}" fill="none" stroke="${col(o.stroke)}" stroke-width="${o.sw}" stroke-linecap="round" stroke-linejoin="round"/>`;
      }
      break;
    }
    case 'rect': {
      const r = o.round ? Math.min(28, Math.min(o.w, o.h) * 0.22) : 0;
      inner = `<rect x="${r2(-hw)}" y="${r2(-hh)}" width="${r2(o.w)}" height="${r2(o.h)}" rx="${r2(r)}" fill="${fill}" ${st}/>` + svgLabel(o, col(o.stroke));
      break;
    }
    case 'ellipse': inner = `<ellipse rx="${r2(hw)}" ry="${r2(hh)}" fill="${fill}" ${st}/>` + svgLabel(o, col(o.stroke)); break;
    case 'diamond': case 'triangle':
      inner = `<polygon points="${polyOf(o).map(p => r2(p[0]) + ',' + r2(p[1])).join(' ')}" fill="${fill}" ${st}/>` + svgLabel(o, col(o.stroke)); break;
    case 'sticky':
      inner = `<rect x="${r2(-hw + 4)}" y="${r2(-hh + 8)}" width="${r2(o.w)}" height="${r2(o.h)}" fill="rgba(0,0,0,.12)"/><rect x="${r2(-hw)}" y="${r2(-hh)}" width="${r2(o.w)}" height="${r2(o.h)}" fill="${o.fill}"/>` + svgLabel(o, '#1e1e1e'); break;
    case 'text': inner = svgLabel(o, col(o.stroke)); break;
    case 'image': inner = `<image x="${r2(-hw)}" y="${r2(-hh)}" width="${r2(o.w)}" height="${r2(o.h)}" href="${S.assets[o.asset] || ''}" preserveAspectRatio="none"/>`; break;
  }
  return `<g transform="${tf}"${op}>${inner}</g>`;
}
function exportSVG(objs, { pad = 24, transparent = false } = {}) {
  const b = exportBounds(objs, pad); if (!b) return null;
  const bg = transparent ? '' : `<rect x="${r2(b.x)}" y="${r2(b.y)}" width="${r2(b.w)}" height="${r2(b.h)}" fill="${bgInfo().c}"/>`;
  return `<?xml version="1.0" encoding="UTF-8"?>\n<svg xmlns="http://www.w3.org/2000/svg" width="${Math.ceil(b.w)}" height="${Math.ceil(b.h)}" viewBox="${r2(b.x)} ${r2(b.y)} ${r2(b.w)} ${r2(b.h)}">${bg}${objs.map(objToSVG).join('')}</svg>`;
}

/* ===== Sérialisation ===== */
const jsonRep = (k, v) => (k[0] === '_' ? undefined : v);
function usedAssets() {
  const out = {};
  for (const o of S.objects) if (o.asset && S.assets[o.asset]) out[o.asset] = S.assets[o.asset];
  return out;
}
function serialize() {
  return {
    app: 'tableau-blanc', version: 1, name: S.name, bg: S.bg, pattern: S.pattern, cam: { ...S.cam },
    objects: JSON.parse(JSON.stringify(S.objects, jsonRep)), assets: usedAssets(),
  };
}
