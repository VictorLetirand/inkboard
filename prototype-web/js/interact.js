'use strict';
/* ===== Historique ===== */
const H = { stack: [], idx: -1 };
const snap = () => JSON.stringify(S.objects, jsonRep);
function resetHistory() { H.stack = [snap()]; H.idx = 0; updateHistoryUI(); }
function commit() {
  const s = snap();
  if (H.stack[H.idx] === s) return;
  H.stack.length = H.idx + 1; H.stack.push(s);
  if (H.stack.length > 150) H.stack.shift();
  H.idx = H.stack.length - 1;
  afterChange();
}
function restoreHistory(i) {
  H.idx = i; S.objects = JSON.parse(H.stack[i]);
  for (const id of [...S.sel]) if (!byId(id)) S.sel.delete(id);
  afterChange(); requestRender();
}
function undo() { if (S.editing) return endEdit(); if (H.idx > 0) restoreHistory(H.idx - 1); }
function redo() { if (S.editing) return; if (H.idx < H.stack.length - 1) restoreHistory(H.idx + 1); }

/* ===== Création d'objets ===== */
function newObj(type, props) {
  const s = S.style;
  return Object.assign({ id: uid(), type, x: 0, y: 0, w: 0, h: 0, angle: 0, stroke: s.stroke, fill: 'transparent', sw: s.sw, dash: s.dash, opacity: s.opacity }, props);
}
const textProps = () => ({ text: '', font: S.style.font, fontSize: S.style.fontSize, align: S.style.align });
function newShape(type, r) {
  const o = newObj(type, { x: r.x, y: r.y, w: r.w, h: r.h });
  if (type === 'sticky') Object.assign(o, { fill: S.style.noteFill, sw: 0, ...textProps(), align: 'center' });
  else { o.fill = S.style.fill; Object.assign(o, textProps(), { align: 'center' }); if (type === 'rect') o.round = S.style.round; }
  return o;
}
function newPen(pts, hl) {
  const o = newObj('pen', { dash: 'solid', hl: !!hl });
  return setAbsPoints(o, pts.length === 1 ? [pts[0], pts[0]] : pts);
}
function newLine(a, b) {
  const o = newObj('line', { arrowEnd: S.tool === 'arrow' && /end|both/.test(S.style.heads), arrowStart: S.tool === 'arrow' && /start|both/.test(S.style.heads) });
  return setAbsPoints(o, [a, b]);
}
const snapV = v => (S.snap ? Math.round(v / GRID) * GRID : v);
const snapP = p => ({ x: snapV(p.x), y: snapV(p.y) });
const clone = o => JSON.parse(JSON.stringify(o, jsonRep));
const unlocked = o => !o.locked;

function cloneObjs(list, dx, dy) {
  const gm = {};
  return list.map(o => {
    const c = clone(o); c.id = uid(); c.x += dx; c.y += dy;
    if (c.gid) { gm[c.gid] = gm[c.gid] || uid(); c.gid = gm[c.gid]; }
    return c;
  });
}

/* ===== Outils ===== */
function setTool(id) {
  if (id === 'image') { $('#fileImg').click(); return; }
  if (S.editing) endEdit();
  const prev = S.tool;
  if (prev === 'highlighter' && id !== 'highlighter') {
    S.hlStyle = { stroke: S.style.stroke, sw: S.style.sw, opacity: S.style.opacity }; Object.assign(S.style, S.penStyle || {});
  } else if (id === 'highlighter' && prev !== 'highlighter') {
    S.penStyle = { stroke: S.style.stroke, sw: S.style.sw, opacity: S.style.opacity }; Object.assign(S.style, S.hlStyle);
  }
  S.tool = id;
  if (id !== 'select' && id !== 'hand') S.sel = new Set();
  S.hover = null;
  updateToolbar(); updatePanel(); setCursor(); requestRender();
}
function afterCreate(o, edit) {
  S.objects.push(o); S.sel = new Set([o.id]);
  commit();
  if (!S.toolLock) setTool('select'); else S.sel = new Set();
  if (edit) { S.sel = new Set([o.id]); startEdit(o, false); }
  updatePanel(); requestRender();
}
function setCursor(c) {
  if (!c) {
    c = { select: 'default', hand: 'grab', text: 'text', eraser: 'cell' }[S.tool] || 'crosshair';
    if (P.space) c = 'grab';
  }
  cv.style.cursor = c;
}

/* ===== Zoom ===== */
function zoomAt(sx, sy, nz) {
  nz = clamp(nz, 0.05, 8);
  const w = s2w(sx, sy);
  S.cam.z = nz; S.cam.x = sx - w.x * nz; S.cam.y = sy - w.y * nz;
  requestRender(); viewChanged();
}
const zoomBy = f => zoomAt(VW / 2, VH / 2, S.cam.z * f);
function fitView(onlySel) {
  const list = onlySel && S.sel.size ? selObjs() : S.objects;
  const b = unionBox(list);
  if (!b) { S.cam = { x: VW / 2, y: VH / 2, z: 1 }; requestRender(); viewChanged(); return; }
  const z = clamp(Math.min((VW - 120) / Math.max(b.w, 1), (VH - 180) / Math.max(b.h, 1)), 0.05, 2);
  S.cam.z = z; S.cam.x = VW / 2 - (b.x + b.w / 2) * z; S.cam.y = VH / 2 - (b.y + b.h / 2) * z;
  requestRender(); viewChanged();
}
function resetZoom() { zoomAt(VW / 2, VH / 2, 1); }

/* ===== Pointeurs ===== */
const P = { ptrs: new Map(), drag: null, space: false, pinch: null, w: { x: 0, y: 0 } };
const CURSORS = { nw: 'nwse-resize', se: 'nwse-resize', ne: 'nesw-resize', sw: 'nesw-resize', n: 'ns-resize', s: 'ns-resize', e: 'ew-resize', w: 'ew-resize', rot: 'grab', p0: 'crosshair', p1: 'crosshair' };

cv.addEventListener('pointerdown', onDown);
window.addEventListener('pointermove', onMove);
window.addEventListener('pointerup', onUp);
window.addEventListener('pointercancel', onUp);

function onDown(e) {
  if (e.button === 2) return;
  closeMenus();
  if (S.editing) endEdit();
  try { cv.setPointerCapture(e.pointerId); } catch (_) {}
  P.ptrs.set(e.pointerId, { x: e.clientX, y: e.clientY });
  if (P.ptrs.size === 2) return startPinch();
  if (P.ptrs.size > 2) return;
  const sp = { x: e.clientX, y: e.clientY }, w = s2w(sp.x, sp.y);
  if (e.button === 1 || S.tool === 'hand' || P.space) {
    P.drag = { mode: 'pan', sx: sp.x, sy: sp.y, cam: { ...S.cam } }; setCursor('grabbing'); return;
  }
  switch (S.tool) {
    case 'select': return downSelect(e, sp, w);
    case 'pen': case 'highlighter':
      P.drag = { mode: 'draw', pts: [[w.x, w.y]], hl: S.tool === 'highlighter' };
      S.draft = newPen(P.drag.pts, P.drag.hl); break;
    case 'eraser': P.drag = { mode: 'erase', set: new Set(), last: w }; eraseAt(w, w); break;
    case 'laser': P.drag = { mode: 'laser' }; S.laser.push({ x: w.x, y: w.y, t: performance.now(), brk: true }); break;
    case 'text': P.drag = { mode: 'textclick', sx: sp.x, sy: sp.y, w }; break;
    default: { const s = snapP(w); P.drag = { mode: 'create', tool: S.tool, start: s, sx: sp.x, sy: sp.y }; }
  }
  requestRender();
}

function downSelect(e, sp, w) {
  const h = hitHandle(w.x, w.y);
  if (h) return startHandleDrag(h, w);
  const o = pickAt(w.x, w.y, 6 / S.cam.z);
  if (o) {
    const ids = expandGroups(new Set([o.id]));
    if (e.shiftKey) {
      if (S.sel.has(o.id)) ids.forEach(i => S.sel.delete(i)); else ids.forEach(i => S.sel.add(i));
      P.drag = { mode: 'none' };
    } else {
      const was = S.sel.has(o.id);
      if (!was) S.sel = ids;
      P.drag = { mode: 'move', sx: sp.x, sy: sp.y, snap: selObjs().filter(unlocked).map(clone), moved: false, collapse: was ? o : null, alt: e.altKey };
    }
  } else {
    if (!e.shiftKey) S.sel = new Set();
    P.drag = { mode: 'marquee', start: w, base: new Set(S.sel) };
    S.marquee = { x0: w.x, y0: w.y, x1: w.x, y1: w.y };
  }
  updatePanel(); requestRender();
}

function startHandleDrag(h, w) {
  const sb = selectionBox();
  if (h.id === 'rot') {
    const b = sb.box, c = { x: b.x + b.w / 2, y: b.y + b.h / 2 };
    P.drag = { mode: 'rotate', c, a0: Math.atan2(w.y - c.y, w.x - c.x), snap: selObjs().filter(unlocked).map(clone) };
  } else if (h.id[0] === 'p') {
    P.drag = { mode: 'endpoint', idx: +h.id[1], id: sb.o.id };
  } else {
    P.drag = { mode: 'resize', h: h.id, box: { ...sb.box }, snap: selObjs().filter(unlocked).map(clone), multi: sb.kind === 'multi' };
  }
}

function startPinch() {
  cancelDrag();
  const [a, b] = [...P.ptrs.values()];
  const d = Math.hypot(a.x - b.x, a.y - b.y) || 1, mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
  P.pinch = { d, z: S.cam.z, w: s2w(mid.x, mid.y) };
}
function cancelDrag() {
  const d = P.drag;
  if (d && d.mode === 'erase') { for (const o of S.objects) delete o._ghost; }
  if (d && (d.mode === 'move' || d.mode === 'resize' || d.mode === 'rotate' || d.mode === 'endpoint')) commit();
  S.draft = null; S.marquee = null; P.drag = null; requestRender();
}

function onMove(e) {
  if (P.ptrs.has(e.pointerId)) P.ptrs.set(e.pointerId, { x: e.clientX, y: e.clientY });
  if (P.pinch && P.ptrs.size >= 2) {
    const [a, b] = [...P.ptrs.values()];
    const d = Math.hypot(a.x - b.x, a.y - b.y) || 1, mid = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
    const z = clamp(P.pinch.z * d / P.pinch.d, 0.05, 8);
    S.cam.z = z; S.cam.x = mid.x - P.pinch.w.x * z; S.cam.y = mid.y - P.pinch.w.y * z;
    requestRender(); viewChanged(); return;
  }
  const sp = { x: e.clientX, y: e.clientY }, w = s2w(sp.x, sp.y);
  if (e.target === cv || P.drag) P.w = w;
  const d = P.drag;
  if (!d) return hover(e, w);
  const z = S.cam.z;
  switch (d.mode) {
    case 'pan':
      S.cam.x = d.cam.x + (sp.x - d.sx); S.cam.y = d.cam.y + (sp.y - d.sy); requestRender(); viewChanged(); break;
    case 'draw': {
      const evs = e.getCoalescedEvents ? e.getCoalescedEvents() : [e];
      for (const ce of (evs.length ? evs : [e])) {
        const p = s2w(ce.clientX, ce.clientY), l = d.pts[d.pts.length - 1];
        if (Math.hypot(p.x - l[0], p.y - l[1]) * z >= 1.2) d.pts.push([p.x, p.y]);
      }
      S.draft = newPen(d.pts, d.hl); requestRender(); break;
    }
    case 'create': moveCreate(d, e, w); break;
    case 'move': moveDrag(d, e, sp); break;
    case 'resize': resizeDrag(d, e, w); break;
    case 'rotate': {
      let da = Math.atan2(w.y - d.c.y, w.x - d.c.x) - d.a0;
      if (e.shiftKey) { const base = d.snap.length === 1 ? d.snap[0].angle || 0 : 0, st = Math.PI / 12; da = Math.round((base + da) / st) * st - base; }
      for (const s of d.snap) {
        const o = replaceObj(s);
        if (d.snap.length === 1) rotateAround(o, o.x + o.w / 2, o.y + o.h / 2, da); else rotateAround(o, d.c.x, d.c.y, da);
      }
      requestRender(); break;
    }
    case 'endpoint': {
      const o = byId(d.id); if (!o) break;
      const pts = absPts(o); let p = [w.x, w.y];
      const other = pts[1 - d.idx];
      if (e.shiftKey) {
        const a = Math.atan2(p[1] - other[1], p[0] - other[0]), l = Math.hypot(p[0] - other[0], p[1] - other[1]), st = Math.PI / 12, na = Math.round(a / st) * st;
        p = [other[0] + l * Math.cos(na), other[1] + l * Math.sin(na)];
      } else if (S.snap) p = [snapV(p[0]), snapV(p[1])];
      pts[d.idx] = p; setAbsPoints(o, pts); requestRender(); break;
    }
    case 'marquee': {
      d.cur = w; S.marquee = { x0: d.start.x, y0: d.start.y, x1: w.x, y1: w.y };
      const x0 = Math.min(d.start.x, w.x), x1 = Math.max(d.start.x, w.x), y0 = Math.min(d.start.y, w.y), y1 = Math.max(d.start.y, w.y);
      const contained = w.x >= d.start.x, ids = new Set(d.base);
      for (const o of S.objects) {
        const b = bboxOf(o);
        const inside = b.x >= x0 && b.y >= y0 && b.x + b.w <= x1 && b.y + b.h <= y1;
        const touch = !(b.x > x1 || b.y > y1 || b.x + b.w < x0 || b.y + b.h < y0);
        if (contained ? inside : touch) ids.add(o.id);
      }
      S.sel = expandGroups(ids); requestRender(); break;
    }
    case 'erase': eraseAt(d.last, w); d.last = w; break;
    case 'laser': S.laser.push({ x: w.x, y: w.y, t: performance.now() }); requestRender(); break;
    case 'textclick': break;
  }
}

function replaceObj(s) {
  const i = S.objects.findIndex(o => o.id === s.id), c = clone(s);
  if (i >= 0) S.objects[i] = c;
  return c;
}

function moveCreate(d, e, w) {
  const cur = snapP(w), a = d.start;
  if (d.tool === 'line' || d.tool === 'arrow') {
    let b = [cur.x, cur.y];
    if (e.shiftKey) {
      const ang = Math.atan2(b[1] - a.y, b[0] - a.x), l = Math.hypot(b[0] - a.x, b[1] - a.y), st = Math.PI / 12, na = Math.round(ang / st) * st;
      b = [a.x + l * Math.cos(na), a.y + l * Math.sin(na)];
    }
    S.draft = newLine([a.x, a.y], b);
  } else {
    let dx = cur.x - a.x, dy = cur.y - a.y;
    if (e.shiftKey) { const m = Math.max(Math.abs(dx), Math.abs(dy)); dx = Math.sign(dx || 1) * m; dy = Math.sign(dy || 1) * m; }
    let r;
    if (e.altKey) r = { x: a.x - Math.abs(dx), y: a.y - Math.abs(dy), w: Math.abs(dx) * 2, h: Math.abs(dy) * 2 };
    else r = { x: Math.min(a.x, a.x + dx), y: Math.min(a.y, a.y + dy), w: Math.abs(dx), h: Math.abs(dy) };
    S.draft = newShape(d.tool, r);
  }
  requestRender();
}

function moveDrag(d, e, sp) {
  const ex = sp.x - d.sx, ey = sp.y - d.sy;
  if (!d.moved) {
    if (Math.hypot(ex, ey) < 3) return;
    d.moved = true;
    if (d.alt) {
      const copies = cloneObjs(d.snap, 0, 0);
      S.objects.push(...copies); S.sel = new Set(copies.map(c => c.id)); d.snap = copies.map(clone);
    }
  }
  let dx = ex / S.cam.z, dy = ey / S.cam.z;
  if (S.snap) { const b = unionBox(d.snap); dx = snapV(b.x + dx) - b.x; dy = snapV(b.y + dy) - b.y; }
  if (e.shiftKey) { if (Math.abs(ex) > Math.abs(ey)) dy = 0; else dx = 0; }
  for (const s of d.snap) { const o = byId(s.id); if (o) { o.x = s.x + dx; o.y = s.y + dy; } }
  requestRender();
}

function resizeDrag(d, e, w) {
  const B = d.box, p = toLocal(B, w.x, w.y), id = d.h, min = 4;
  const hasE = id.includes('e'), hasW = id.includes('w'), hasN = id.includes('n'), hasS = id.includes('s');
  let left = -B.w / 2, right = B.w / 2, top = -B.h / 2, bottom = B.h / 2;
  if (hasE) right = Math.max(left + min, p.x);
  if (hasW) left = Math.min(right - min, p.x);
  if (hasS) bottom = Math.max(top + min, p.y);
  if (hasN) top = Math.min(bottom - min, p.y);
  let nw = right - left, nh = bottom - top;
  const keepRatio = id.length === 2 && (e.shiftKey || d.snap.some(s => s.type === 'image' || s.type === 'text'));
  if (keepRatio && B.w > 0 && B.h > 0) {
    const k = Math.max(nw / B.w, nh / B.h); nw = B.w * k; nh = B.h * k;
    if (hasE) right = left + nw; else left = right - nw;
    if (hasS) bottom = top + nh; else top = bottom - nh;
  }
  const cw = toWorld(B, (left + right) / 2, (top + bottom) / 2);
  if (!d.multi) {
    const s = d.snap[0]; if (!s) return;
    const o = replaceObj(s);
    o.x = cw.x - nw / 2; o.y = cw.y - nh / 2; o.w = nw; o.h = nh;
    if (o.type === 'text') { o.fontSize = Math.max(6, s.fontSize * (nh / (B.h || 1))); fitText(o); }
  } else {
    const nb = { x: cw.x - nw / 2, y: cw.y - nh / 2 }, sx = nw / (B.w || 1), sy = nh / (B.h || 1);
    for (const s of d.snap) {
      const o = replaceObj(s);
      const ncx = nb.x + (s.x + s.w / 2 - B.x) * sx, ncy = nb.y + (s.y + s.h / 2 - B.y) * sy;
      o.w = s.w * sx; o.h = s.h * sy; o.x = ncx - o.w / 2; o.y = ncy - o.h / 2;
      if (o.type === 'text') { o.fontSize = Math.max(6, s.fontSize * Math.min(sx, sy)); fitText(o); o.x = ncx - o.w / 2; o.y = ncy - o.h / 2; }
    }
  }
  requestRender();
}

function eraseAt(a, b) {
  const tol = 8 / S.cam.z, dist = Math.hypot(b.x - a.x, b.y - a.y), n = Math.max(1, Math.ceil(dist / (tol * 0.8)));
  for (let i = 0; i <= n; i++) {
    const x = a.x + ((b.x - a.x) * i) / n, y = a.y + ((b.y - a.y) * i) / n;
    const o = pickAt(x, y, tol);
    if (o && !o.locked && !P.drag.set.has(o.id)) { P.drag.set.add(o.id); o._ghost = true; }
  }
  requestRender();
}

function hover(e, w) {
  if (S.tool !== 'select' || e.target !== cv || P.space) return;
  const h = hitHandle(w.x, w.y);
  let id = null, cur = 'default';
  if (h) cur = CURSORS[h.id];
  else {
    const o = pickAt(w.x, w.y, 6 / S.cam.z);
    if (o) { id = o.id; cur = S.sel.has(o.id) && !o.locked ? 'move' : 'default'; }
  }
  setCursor(cur);
  if (id !== S.hover) { S.hover = id; requestRender(); }
}

function onUp(e) {
  P.ptrs.delete(e.pointerId);
  if (P.pinch) { if (P.ptrs.size < 2) P.pinch = null; return; }
  const d = P.drag; if (!d) return;
  P.drag = null;
  const sp = { x: e.clientX, y: e.clientY }, w = s2w(sp.x, sp.y);
  switch (d.mode) {
    case 'pan': setCursor(); break;
    case 'draw': {
      S.draft = null;
      const pts = d.pts.length > 2 ? rdp(d.pts, 0.45 / S.cam.z) : d.pts;
      S.objects.push(newPen(pts, d.hl)); commit(); break;
    }
    case 'create': finishCreate(d, e); break;
    case 'move':
      if (d.moved) commit();
      else if (d.collapse && !e.shiftKey) S.sel = expandGroups(new Set([d.collapse.id]));
      updatePanel(); break;
    case 'resize': case 'rotate': case 'endpoint': commit(); break;
    case 'marquee': S.marquee = null; updatePanel(); break;
    case 'erase': {
      for (const o of S.objects) delete o._ghost;
      if (d.set.size) { S.objects = S.objects.filter(o => !d.set.has(o.id)); for (const id of d.set) S.sel.delete(id); commit(); updatePanel(); }
      break;
    }
    case 'textclick':
      if (Math.hypot(sp.x - d.sx, sp.y - d.sy) < 6) {
        const o = newObj('text', { x: d.w.x, y: d.w.y - S.style.fontSize * 0.65, sw: 0, ...textProps() });
        fitText(o); S.objects.push(o); S.sel = new Set([o.id]);
        setTimeout(() => startEdit(o, true), 0);
      }
      break;
    case 'none': updatePanel(); break;
  }
  requestRender();
}

function finishCreate(d, e) {
  const draft = S.draft; S.draft = null;
  const t = d.tool;
  if (t === 'line' || t === 'arrow') {
    if (draft && Math.hypot(draft.w, draft.h) >= 6) afterCreate(draft, false);
    return;
  }
  const defs = { rect: [150, 100], ellipse: [150, 100], diamond: [150, 120], triangle: [140, 120], sticky: [200, 200] }[t];
  let o = draft;
  const tooSmall = !o || (o.w < 6 && o.h < 6);
  if (tooSmall) o = newShape(t, { x: d.start.x - defs[0] / 2, y: d.start.y - defs[1] / 2, w: defs[0], h: defs[1] });
  else if (t === 'sticky') { o.w = Math.max(o.w, 80); o.h = Math.max(o.h, 80); }
  else { o.w = Math.max(o.w, 6); o.h = Math.max(o.h, 6); }
  afterCreate(o, t === 'sticky');
}

/* ===== Molette ===== */
cv.addEventListener('wheel', e => {
  e.preventDefault(); closeMenus();
  const mouseWheel = e.deltaX === 0 && Number.isInteger(e.deltaY) && Math.abs(e.deltaY) >= 50 && e.deltaMode === 0;
  if (e.ctrlKey || e.metaKey || mouseWheel) {
    const f = Math.exp(-e.deltaY * (e.ctrlKey ? 0.01 : 0.0015));
    zoomAt(e.clientX, e.clientY, S.cam.z * f);
  } else {
    S.cam.x -= e.shiftKey ? e.deltaY : e.deltaX; S.cam.y -= e.shiftKey ? 0 : e.deltaY;
    requestRender(); viewChanged();
  }
}, { passive: false });

/* ===== Double-clic & menu contextuel ===== */
cv.addEventListener('dblclick', e => {
  if (S.tool !== 'select') return;
  const w = s2w(e.clientX, e.clientY), o = pickAt(w.x, w.y, 6 / S.cam.z);
  if (o) { if (LABELED.includes(o.type)) { S.sel = new Set([o.id]); startEdit(o, false); } return; }
  const t = newObj('text', { x: w.x, y: w.y - S.style.fontSize * 0.65, sw: 0, ...textProps() });
  fitText(t); S.objects.push(t); S.sel = new Set([t.id]); startEdit(t, true);
});
cv.addEventListener('contextmenu', e => {
  e.preventDefault();
  const w = s2w(e.clientX, e.clientY), o = pickAt(w.x, w.y, 6 / S.cam.z);
  if (o && !S.sel.has(o.id)) { S.sel = expandGroups(new Set([o.id])); updatePanel(); requestRender(); }
  else if (!o) { S.sel = new Set(); updatePanel(); requestRender(); }
  showContextMenu(e.clientX, e.clientY);
});

/* ===== Édition de texte ===== */
const ed = $('#editor');
function startEdit(o, isNew) {
  if (S.editing) endEdit();
  S.editing = o.id; S.sel = new Set([o.id]);
  ed.hidden = false; ed.value = o.text || '';
  positionEditor();
  ed.focus(); ed.setSelectionRange(ed.value.length, ed.value.length);
  updatePanel(); requestRender();
}
function positionEditor() {
  const o = byId(S.editing); if (!o) return;
  const li = labelInfo(o), b = li.box, z = S.cam.z;
  const tl = w2s(...Object.values(toWorld(o, b.x, b.y)));
  const isText = o.type === 'text';
  ed.wrap = isText ? 'off' : 'soft';
  ed.style.whiteSpace = isText ? 'pre' : 'pre-wrap';
  ed.style.left = tl.x + 'px'; ed.style.top = tl.y + 'px';
  ed.style.transformOrigin = '0 0';
  ed.style.transform = o.angle ? `rotate(${o.angle}rad)` : 'none';
  ed.style.font = fontStr(o, z);
  ed.style.lineHeight = li.lh * z + 'px';
  ed.style.textAlign = o.align || 'left';
  ed.style.color = o.type === 'sticky' ? '#1e1e1e' : col(o.stroke);
  const pt = isText ? 0 : Math.max(0, (b.h - li.total) / 2) * z;
  ed.style.paddingTop = pt + 'px';
  ed.style.width = b.w * z + (isText ? 12 : 0) + 'px';
  ed.style.height = Math.max(b.h * z, li.lh * z) + 'px';
}
function endEdit() {
  if (!S.editing) return;
  const id = S.editing; S.editing = null;
  const o = byId(id);
  if (o) {
    o.text = ed.value;
    if (o.type === 'text' && !o.text.trim()) { S.objects = S.objects.filter(x => x.id !== id); S.sel.delete(id); }
    else ensureFit(o);
  }
  ed.hidden = true; ed.value = '';
  commit(); updatePanel(); requestRender();
}
ed.addEventListener('input', () => {
  const o = byId(S.editing); if (!o) return;
  o.text = ed.value; ensureFit(o); positionEditor(); requestRender();
});
ed.addEventListener('keydown', e => {
  e.stopPropagation();
  if (e.key === 'Escape' || (e.key === 'Enter' && (e.ctrlKey || e.metaKey))) { e.preventDefault(); endEdit(); }
});
ed.addEventListener('blur', () => endEdit());

/* ===== Actions sur la sélection ===== */
function deleteSel() {
  const ids = new Set(selObjs().filter(unlocked).map(o => o.id)); if (!ids.size) return;
  S.objects = S.objects.filter(o => !ids.has(o.id)); S.sel = new Set(); commit(); updatePanel(); requestRender();
}
function duplicateSel() {
  const objs = selObjs(); if (!objs.length) return;
  const copies = cloneObjs(objs, 24, 24);
  S.objects.push(...copies); S.sel = new Set(copies.map(c => c.id)); commit(); updatePanel(); requestRender();
}
function selectAll() { S.sel = new Set(S.objects.map(o => o.id)); updatePanel(); requestRender(); }
function groupSel() {
  const objs = selObjs(); if (objs.length < 2) return;
  const g = uid(); objs.forEach(o => (o.gid = g)); commit(); updatePanel();
}
function ungroupSel() {
  const objs = selObjs(); if (!objs.some(o => o.gid)) return;
  objs.forEach(o => delete o.gid); commit(); updatePanel();
}
function toggleLock() {
  const objs = selObjs(); if (!objs.length) return;
  const to = !objs.every(o => o.locked); objs.forEach(o => (o.locked = to)); commit(); updatePanel(); requestRender();
  toast(to ? 'Objets verrouillés' : 'Objets déverrouillés');
}
function reorder(mode) {
  const ids = S.sel, a = S.objects;
  if (!ids.size) return;
  if (mode === 'front') S.objects = [...a.filter(o => !ids.has(o.id)), ...a.filter(o => ids.has(o.id))];
  else if (mode === 'back') S.objects = [...a.filter(o => ids.has(o.id)), ...a.filter(o => !ids.has(o.id))];
  else if (mode === 'forward') { for (let i = a.length - 2; i >= 0; i--) if (ids.has(a[i].id) && !ids.has(a[i + 1].id)) [a[i], a[i + 1]] = [a[i + 1], a[i]]; }
  else { for (let i = 1; i < a.length; i++) if (ids.has(a[i].id) && !ids.has(a[i - 1].id)) [a[i], a[i - 1]] = [a[i - 1], a[i]]; }
  commit(); requestRender();
}
function alignSel(mode) {
  const objs = selObjs().filter(unlocked); if (objs.length < 2) return;
  const u = unionBox(objs);
  for (const o of objs) {
    const b = bboxOf(o); let dx = 0, dy = 0;
    if (mode === 'l') dx = u.x - b.x; else if (mode === 'c') dx = u.x + u.w / 2 - (b.x + b.w / 2); else if (mode === 'r') dx = u.x + u.w - (b.x + b.w);
    else if (mode === 't') dy = u.y - b.y; else if (mode === 'm') dy = u.y + u.h / 2 - (b.y + b.h / 2); else if (mode === 'b') dy = u.y + u.h - (b.y + b.h);
    o.x += dx; o.y += dy;
  }
  commit(); requestRender();
}
function distributeSel(axis) {
  const objs = selObjs().filter(unlocked); if (objs.length < 3) return;
  const hz = axis === 'h', items = objs.map(o => ({ o, b: bboxOf(o) })).sort((p, q) => (hz ? p.b.x - q.b.x : p.b.y - q.b.y));
  const first = items[0].b, last = items[items.length - 1].b;
  const span = hz ? last.x + last.w - first.x : last.y + last.h - first.y;
  const sizes = items.reduce((s, i) => s + (hz ? i.b.w : i.b.h), 0), gap = (span - sizes) / (items.length - 1);
  let pos = hz ? first.x : first.y;
  for (const it of items) {
    if (hz) it.o.x += pos - it.b.x; else it.o.y += pos - it.b.y;
    pos += (hz ? it.b.w : it.b.h) + gap;
  }
  commit(); requestRender();
}

/* ===== Propriétés ===== */
const PROP_TYPES = {
  stroke: ['pen', 'line', 'text', ...SHAPES], fill: [...SHAPES, 'sticky'], sw: ['pen', 'line', ...SHAPES], dash: ['line', ...SHAPES],
  opacity: ['pen', 'line', 'text', 'sticky', 'image', ...SHAPES], round: ['rect'],
  font: ['text', 'sticky', ...SHAPES], fontSize: ['text', 'sticky', ...SHAPES], align: ['text', 'sticky', ...SHAPES],
};
function applyProp(k, v) {
  const objs = selObjs().filter(unlocked);
  if (k === 'fill') S.style[objs.length && objs.every(o => o.type === 'sticky') || (!objs.length && S.tool === 'sticky') ? 'noteFill' : 'fill'] = v;
  else if (k === 'heads') S.style.heads = v;
  else S.style[k] = v;
  for (const o of objs) {
    if (k === 'heads') { if (o.type === 'line') { o.arrowStart = /start|both/.test(v); o.arrowEnd = /end|both/.test(v); } continue; }
    if (!PROP_TYPES[k] || !PROP_TYPES[k].includes(o.type)) continue;
    o[k] = v;
    if (k === 'font' || k === 'fontSize') ensureFit(o);
  }
  if (objs.length) commit();
  updatePanel(); requestRender();
}

/* ===== Presse-papiers ===== */
let clip = null;
function copySel() {
  const objs = selObjs(); if (!objs.length) return false;
  const assets = {}; objs.forEach(o => { if (o.asset) assets[o.asset] = S.assets[o.asset]; });
  clip = { objs: objs.map(clone), assets, n: 0 }; return true;
}
function pasteClip() {
  if (!clip) return;
  clip.n++;
  const copies = cloneObjs(clip.objs, 24 * clip.n, 24 * clip.n);
  Object.assign(S.assets, clip.assets);
  S.objects.push(...copies); S.sel = new Set(copies.map(c => c.id)); commit(); updatePanel(); requestRender();
}
const typing = t => t && (t.tagName === 'TEXTAREA' || t.tagName === 'INPUT' || t.tagName === 'SELECT' || t.isContentEditable);
document.addEventListener('copy', e => {
  if (typing(e.target) || !S.sel.size) return;
  e.preventDefault(); copySel(); e.clipboardData.setData('text/plain', '__tb_clip__'); toast('Copié');
});
document.addEventListener('cut', e => {
  if (typing(e.target) || !S.sel.size) return;
  e.preventDefault(); copySel(); e.clipboardData.setData('text/plain', '__tb_clip__'); deleteSel();
});
document.addEventListener('paste', e => {
  if (typing(e.target)) return;
  e.preventDefault();
  const files = [...(e.clipboardData.files || [])].filter(f => f.type.startsWith('image/'));
  if (files.length) return addImageFiles(files, P.w);
  const txt = e.clipboardData.getData('text/plain');
  if (txt && txt !== '__tb_clip__') {
    const o = newObj('text', { x: P.w.x, y: P.w.y, sw: 0, ...textProps(), text: txt.slice(0, 5000) });
    fitText(o); S.objects.push(o); S.sel = new Set([o.id]); commit(); updatePanel(); requestRender();
  } else pasteClip();
});

/* ===== Images ===== */
function readImage(file) {
  return new Promise(res => {
    const fr = new FileReader();
    fr.onload = () => {
      const img = new Image();
      img.onload = () => {
        let w = img.naturalWidth || 300, h = img.naturalHeight || 200, url = fr.result;
        const max = 2200;
        if (Math.max(w, h) > max && file.type !== 'image/svg+xml') {
          const k = max / Math.max(w, h), c = document.createElement('canvas');
          c.width = Math.round(w * k); c.height = Math.round(h * k);
          c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
          url = c.toDataURL(file.type === 'image/png' ? 'image/png' : 'image/jpeg', 0.9); w = c.width; h = c.height;
        }
        res({ url, w, h });
      };
      img.onerror = () => res(null);
      img.src = fr.result;
    };
    fr.onerror = () => res(null);
    fr.readAsDataURL(file);
  });
}
async function addImageFiles(files, at) {
  const c = at || s2w(VW / 2, VH / 2), added = [];
  let i = 0;
  for (const f of files) {
    const r = await readImage(f); if (!r) continue;
    const k = Math.min(1, (VW * 0.5) / S.cam.z / r.w, (VH * 0.6) / S.cam.z / r.h);
    const w = r.w * k, h = r.h * k, aid = uid();
    S.assets[aid] = r.url;
    added.push(newObj('image', { asset: aid, sw: 0, x: c.x - w / 2 + i * 24, y: c.y - h / 2 + i * 24, w, h })); i++;
  }
  if (!added.length) return toast('Image illisible');
  S.objects.push(...added); S.sel = new Set(added.map(o => o.id));
  commit(); if (S.tool !== 'select') setTool('select'); updatePanel(); requestRender();
}
$('#fileImg').addEventListener('change', e => { addImageFiles([...e.target.files]); e.target.value = ''; });

let dragDepth = 0;
window.addEventListener('dragenter', e => { e.preventDefault(); dragDepth++; $('#dropHint').hidden = false; });
window.addEventListener('dragleave', () => { if (--dragDepth <= 0) { dragDepth = 0; $('#dropHint').hidden = true; } });
window.addEventListener('dragover', e => e.preventDefault());
window.addEventListener('drop', e => {
  e.preventDefault(); dragDepth = 0; $('#dropHint').hidden = true;
  const files = [...(e.dataTransfer.files || [])];
  const imgs = files.filter(f => f.type.startsWith('image/'));
  if (imgs.length) addImageFiles(imgs, s2w(e.clientX, e.clientY));
  else if (files[0]) importBoardFile(files[0]);
});

/* ===== Clavier ===== */
let nudgeTimer = 0;
window.addEventListener('keydown', e => {
  if (typing(e.target)) return;
  const mod = e.ctrlKey || e.metaKey, k = e.key.toLowerCase();
  if (e.key === ' ') { if (!P.space) { P.space = true; setCursor('grab'); } e.preventDefault(); return; }
  if (mod) {
    const prevent = () => e.preventDefault();
    switch (k) {
      case 'z': prevent(); e.shiftKey ? redo() : undo(); return;
      case 'y': prevent(); redo(); return;
      case 'a': prevent(); selectAll(); return;
      case 'd': prevent(); duplicateSel(); return;
      case 'g': prevent(); e.shiftKey ? ungroupSel() : groupSel(); return;
      case 's': prevent(); saveFile(); return;
      case 'o': prevent(); $('#fileOpen').click(); return;
      case 'e': prevent(); exportPNG(); return;
      case '0': prevent(); resetZoom(); return;
      case '1': prevent(); fitView(); return;
      case '=': case '+': prevent(); zoomBy(1.2); return;
      case '-': prevent(); zoomBy(1 / 1.2); return;
      case ']': case '}': prevent(); reorder(e.shiftKey ? 'front' : 'forward'); return;
      case '[': case '{': prevent(); reorder(e.shiftKey ? 'back' : 'backward'); return;
      case 'l': if (e.shiftKey) { prevent(); toggleLock(); } return;
    }
    return;
  }
  if (e.key === 'Escape') {
    if (P.drag) cancelDrag(); else { S.sel = new Set(); closeMenus(); closeModal(); setTool('select'); updatePanel(); requestRender(); }
    return;
  }
  if (e.key === 'Delete' || e.key === 'Backspace') { e.preventDefault(); deleteSel(); return; }
  if (e.key === 'Enter') {
    const objs = selObjs();
    if (objs.length === 1 && LABELED.includes(objs[0].type)) { e.preventDefault(); startEdit(objs[0], false); }
    return;
  }
  if (e.key.startsWith('Arrow') && S.sel.size) {
    e.preventDefault();
    const st = e.shiftKey ? 10 : S.snap ? GRID : 1;
    const dx = e.key === 'ArrowLeft' ? -st : e.key === 'ArrowRight' ? st : 0, dy = e.key === 'ArrowUp' ? -st : e.key === 'ArrowDown' ? st : 0;
    selObjs().filter(unlocked).forEach(o => { o.x += dx; o.y += dy; });
    requestRender(); clearTimeout(nudgeTimer); nudgeTimer = setTimeout(commit, 400); return;
  }
  if (e.key === '?') { showHelp(); return; }
  if (k === 'q') { S.toolLock = !S.toolLock; updateToolbar(); toast(S.toolLock ? 'Outil verrouillé' : 'Outil déverrouillé'); return; }
  const t = TOOLS.find(t => t.key === k);
  if (t && !e.altKey) setTool(t.id);
});
window.addEventListener('keyup', e => { if (e.key === ' ') { P.space = false; setCursor(); } });
window.addEventListener('blur', () => { P.space = false; });
window.addEventListener('resize', () => { resizeCanvas(); if (S.editing) positionEditor(); });
