'use strict';
/* ===== Formats d'échange : draw.io, Excalidraw, JSON Canvas, Mermaid ===== */

/* ---------- Utilitaires communs ---------- */
function mk(type, p = {}) {
  const labeled = type === 'text' || type === 'sticky' || SHAPES.includes(type);
  return Object.assign(
    { id: uid(), type, x: 0, y: 0, w: 0, h: 0, angle: 0, stroke: 'ink', fill: 'transparent', sw: type === 'text' ? 0 : 3, dash: 'solid', opacity: 1 },
    labeled ? { text: '', font: 'sans', fontSize: 20, align: type === 'text' ? 'left' : 'center' } : {}, p);
}
const NAMED = { black: '#000000', white: '#ffffff', red: '#ff0000', green: '#008000', blue: '#0000ff', yellow: '#ffff00', gray: '#808080', grey: '#808080', orange: '#ffa500', purple: '#800080' };
function normColor(c, fallback) {
  if (c == null) return fallback;
  c = String(c).trim().toLowerCase();
  if (c === 'none' || c === 'transparent') return 'transparent';
  if (NAMED[c]) return NAMED[c];
  const m = c.match(/#[0-9a-f]{3,8}/);
  if (!m) return fallback;
  let h = m[0];
  if (h.length === 4) h = '#' + h[1] + h[1] + h[2] + h[2] + h[3] + h[3];
  if (h.length === 9) h = h.slice(0, 7);
  return h.length === 7 ? h : fallback;
}
const toInk = c => (c === '#000000' || c === '#1e1e1e' || c === '#191919' ? 'ink' : c);
function nearestBg(hex) {
  const rgb = h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16));
  const t = rgb(normColor(hex, '#ffffff'));
  let best = BACKGROUNDS[0], bd = Infinity;
  for (const b of BACKGROUNDS) { const c = rgb(b.c), d = c.reduce((s, v, i) => s + (v - t[i]) ** 2, 0); if (d < bd) { bd = d; best = b; } }
  return best.id;
}
const fontKey = f => (/mono|courier|consolas|cascadia/i.test(f || '') ? 'mono' : /print|comic|hand|virgil|kalam|caveat|excalifont/i.test(f || '') ? 'hand' : /times|georgia|garamond|serif/i.test(f || '') && !/sans/i.test(f) ? 'serif' : 'sans');
const fontFirst = f => (FONTS[f] || FONTS.sans).split(',')[0].replace(/"/g, '').trim();
const htmlEsc = s => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
function b64(str) {
  const bytes = new TextEncoder().encode(str); let bin = '';
  for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(bin);
}
const dataMime = u => (String(u).match(/^data:([^;,]+)/) || [])[1] || 'image/png';
function htmlToText(h) {
  if (!h) return '';
  h = String(h).replace(/<br\s*\/?>/gi, '\n').replace(/<\/(div|p|li|h\d)>/gi, '\n').replace(/<(div|p|li)[^>]*>/gi, '');
  const d = new DOMParser().parseFromString('<body>' + h + '</body>', 'text/html');
  return (d.body.textContent || '').replace(/\n+$/, '');
}
function boundaryPoint(r, tx, ty) {
  const cx = r.x + r.w / 2, cy = r.y + r.h / 2, dx = tx - cx, dy = ty - cy;
  if (!dx && !dy) return [cx, cy];
  const hw = r.w / 2 || 1, hh = r.h / 2 || 1; let t;
  if (r.kind === 'ellipse') t = 1 / Math.hypot(dx / hw, dy / hh);
  else if (r.kind === 'diamond') t = 1 / (Math.abs(dx) / hw + Math.abs(dy) / hh);
  else t = Math.min(hw / Math.abs(dx || 1e-9), hh / Math.abs(dy || 1e-9));
  return [cx + dx * t, cy + dy * t];
}
/** Polyligne -> segments "line" ; groupés si plusieurs segments. */
function polyline(points, { arrowStart = false, arrowEnd = true, dash = 'solid', sw = 3, stroke = 'ink', opacity = 1 } = {}) {
  const segs = [], gid = points.length > 2 ? uid() : undefined;
  for (let i = 0; i < points.length - 1; i++) {
    const o = mk('line', { sw, dash, stroke, opacity, arrowStart: arrowStart && i === 0, arrowEnd: arrowEnd && i === points.length - 2 });
    if (gid) o.gid = gid;
    segs.push(setAbsPoints(o, [points[i], points[i + 1]]));
  }
  return segs;
}
function textAt(text, cx, cy, fontSize = 15, align = 'center', extra = {}) {
  const t = mk('text', { text, fontSize, align, ...extra });
  fitText(t); t.x = cx - t.w / 2; t.y = cy - t.h / 2; return t;
}
function roundRep(rep) {
  const parts = [];
  if (rep.approx) parts.push(`${rep.approx} forme(s) approximée(s)`);
  if (rep.skipped) parts.push(`${rep.skipped} élément(s) ignoré(s)`);
  return parts.length ? ' — ' + parts.join(', ') : '';
}

/* ---------- Chargement / insertion ---------- */
async function loadImported({ objects, assets = {}, name, bg, rep = {} }, label) {
  if (!objects.length) return toast('Aucun élément reconnu dans ce fichier');
  await saveNow();
  loadDoc({ name: name || 'Importé', bg: bg || S.bg, pattern: S.pattern, objects, assets }, uid());
  fitView(); markDirty(); await saveNow();
  toast(`${label || 'Importé'} : ${objects.length} objet(s)${roundRep(rep)}`);
}
function insertImported(objs, assets) {
  if (!objs.length) return toast('Rien à insérer');
  Object.assign(S.assets, assets || {});
  const b = unionBox(objs), c = s2w(VW / 2, VH / 2), dx = c.x - (b.x + b.w / 2), dy = c.y - (b.y + b.h / 2);
  for (const o of objs) { o.x += dx; o.y += dy; }
  S.objects.push(...objs); S.sel = new Set(objs.map(o => o.id));
  commit(); updatePanel(); fitView(true); requestRender();
}

/* =====================================================================
   draw.io (.drawio, mxGraph XML non compressé)
   ===================================================================== */
function exportDrawioXml(objs) {
  const b = exportBounds(objs, 0) || { x: 0, y: 0, w: 100, h: 100 };
  const ox = 40 - b.x, oy = 40 - b.y;
  let n = 1; const nid = () => 'tb-' + ++n, cells = [];
  const fnt = o => `fontFamily=${fontFirst(o.font)};fontSize=${o.fontSize};align=${o.align || 'center'};`;
  const dash = o => (o.dash === 'dashed' ? 'dashed=1;' : o.dash === 'dotted' ? 'dashed=1;dashPattern=1 4;' : '');
  const geo = (x, y, w, h) => `<mxGeometry x="${r2(x + ox)}" y="${r2(y + oy)}" width="${r2(w)}" height="${r2(h)}" as="geometry"/>`;
  const val = t => esc(htmlEsc(t || '').replace(/\n/g, '<br>'));
  const rot = o => (o.angle ? `rotation=${r2((o.angle * 180) / Math.PI)};` : '');
  const op = o => ((o.opacity ?? 1) < 1 ? `opacity=${Math.round(o.opacity * 100)};` : '');
  const vertex = (value, style, x, y, w, h) => cells.push(`<mxCell id="${nid()}" value="${value}" style="${esc(style)}" vertex="1" parent="1">${geo(x, y, w, h)}</mxCell>`);
  for (const o of objs) {
    const stroke = o.sw > 0 ? `strokeColor=${col(o.stroke)};strokeWidth=${o.sw};` : 'strokeColor=none;';
    const fill = o.fill && o.fill !== 'transparent' ? `fillColor=${col(o.fill)};` : 'fillColor=none;';
    switch (o.type) {
      case 'rect': {
        const arc = o.round ? Math.min(100, Math.round((Math.min(28, Math.min(o.w, o.h) * 0.22) / (Math.min(o.w, o.h) || 1)) * 100)) : 0;
        vertex(val(o.text), `rounded=${o.round ? 1 : 0};${o.round ? 'arcSize=' + arc + ';' : ''}whiteSpace=wrap;html=1;${fill}${stroke}${dash(o)}${fnt(o)}verticalAlign=middle;fontColor=${col(o.stroke)};${rot(o)}${op(o)}`, o.x, o.y, o.w, o.h); break;
      }
      case 'ellipse': case 'diamond': case 'triangle': {
        const shape = { ellipse: 'ellipse;', diamond: 'rhombus;', triangle: 'triangle;direction=north;' }[o.type];
        vertex(val(o.text), `${shape}whiteSpace=wrap;html=1;${fill}${stroke}${dash(o)}${fnt(o)}verticalAlign=middle;fontColor=${col(o.stroke)};${rot(o)}${op(o)}`, o.x, o.y, o.w, o.h); break;
      }
      case 'sticky':
        vertex(val(o.text), `rounded=0;whiteSpace=wrap;html=1;shadow=1;tbType=sticky;fillColor=${o.fill};strokeColor=none;${fnt(o)}verticalAlign=middle;fontColor=#1E1E1E;${rot(o)}${op(o)}`, o.x, o.y, o.w, o.h); break;
      case 'text':
        vertex(val(o.text), `text;html=1;strokeColor=none;fillColor=none;whiteSpace=wrap;verticalAlign=top;spacing=0;${fnt(o)}fontColor=${col(o.stroke)};${rot(o)}${op(o)}`, o.x, o.y, o.w, o.h); break;
      case 'line': {
        const p = absPts(o);
        cells.push(`<mxCell id="${nid()}" value="" style="${esc(`endArrow=${o.arrowEnd ? 'classic' : 'none'};startArrow=${o.arrowStart ? 'classic' : 'none'};endFill=1;startFill=1;html=1;rounded=0;strokeColor=${col(o.stroke)};strokeWidth=${o.sw};${dash(o)}${op(o)}`)}" edge="1" parent="1"><mxGeometry relative="1" as="geometry"><mxPoint x="${r2(p[0][0] + ox)}" y="${r2(p[0][1] + oy)}" as="sourcePoint"/><mxPoint x="${r2(p[1][0] + ox)}" y="${r2(p[1][1] + oy)}" as="targetPoint"/></mxGeometry></mxCell>`);
        break;
      }
      case 'pen': {
        const bb = exportBounds([o], 0), svg = exportSVG([o], { pad: 0, transparent: true });
        vertex('', `shape=image;html=1;imageAspect=0;verticalLabelPosition=bottom;verticalAlign=top;tbType=pen;image=data:image/svg+xml,${b64(svg)};`, bb.x, bb.y, bb.w, bb.h); break;
      }
      case 'image': {
        const url = (S.assets[o.asset] || '').replace(';base64,', ',');
        vertex('', `shape=image;html=1;imageAspect=0;verticalLabelPosition=bottom;verticalAlign=top;image=${url};${rot(o)}${op(o)}`, o.x, o.y, o.w, o.h); break;
      }
    }
  }
  return `<mxfile host="Tableau blanc" agent="Tableau blanc" version="1.0" type="device"><diagram id="tb-1" name="${esc(S.name || 'Page-1')}"><mxGraphModel dx="1200" dy="800" grid="${S.pattern === 'grid' || S.pattern === 'dots' ? 1 : 0}" gridSize="10" guides="1" tooltips="1" connect="1" arrows="1" fold="1" page="0" pageScale="1" pageWidth="1169" pageHeight="827" math="0" shadow="0" background="${bgInfo().c}"><root><mxCell id="0"/><mxCell id="1" parent="0"/>${cells.join('')}</root></mxGraphModel></diagram></mxfile>`;
}

async function inflateDrawio(raw) {
  const bin = atob(raw.replace(/\s+/g, '')), bytes = Uint8Array.from(bin, c => c.charCodeAt(0));
  const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
  return decodeURIComponent(await new Response(stream).text());
}
function dioStyle(str) {
  const m = {}, names = [];
  for (const tok of String(str || '').split(';')) {
    if (!tok) continue;
    const i = tok.indexOf('=');
    if (i < 0) names.push(tok); else m[tok.slice(0, i)] = tok.slice(i + 1);
  }
  return { m, names };
}
function dioImage(v) {
  const mm = /^data:([^;,]+)(;base64)?,(.*)$/s.exec(v || '');
  if (!mm) return null;
  const [, mime, b, payload] = mm;
  if (b || /^[A-Za-z0-9+/=\s]+$/.test(payload)) return `data:${mime};base64,${payload}`;
  return `data:${mime},${payload}`;
}

async function parseDrawio(text, baseName) {
  const xml = new DOMParser().parseFromString(text, 'text/xml');
  if (xml.querySelector('parsererror')) throw new Error('XML invalide');
  let model = xml.querySelector('mxGraphModel'), diagram = xml.querySelector('diagram');
  if (!model) {
    if (!diagram) throw new Error('aucun diagramme draw.io');
    const raw = (diagram.textContent || '').trim();
    let inner;
    try { inner = await inflateDrawio(raw); } catch (_) { throw new Error('diagramme compressé illisible'); }
    model = new DOMParser().parseFromString(inner, 'text/xml').querySelector('mxGraphModel');
    if (!model) throw new Error('modèle draw.io introuvable');
  }
  const rep = { approx: 0, skipped: 0 }, out = [], assets = {};
  const cells = new Map();
  for (const el of model.querySelectorAll('mxCell')) {
    const wrap = el.parentElement && /^(object|UserObject)$/.test(el.parentElement.tagName) ? el.parentElement : null;
    const id = (wrap || el).getAttribute('id'), st = dioStyle(el.getAttribute('style'));
    const gEl = [...el.children].find(c => c.tagName === 'mxGeometry');
    let geo = null;
    if (gEl) {
      geo = { x: +gEl.getAttribute('x') || 0, y: +gEl.getAttribute('y') || 0, w: +gEl.getAttribute('width') || 0, h: +gEl.getAttribute('height') || 0, pts: [] };
      for (const p of gEl.children) {
        if (p.tagName === 'mxPoint') { const q = { x: +p.getAttribute('x') || 0, y: +p.getAttribute('y') || 0 }; if (p.getAttribute('as') === 'sourcePoint') geo.sp = q; else if (p.getAttribute('as') === 'targetPoint') geo.tp = q; }
        else if (p.tagName === 'Array') for (const q of p.children) geo.pts.push({ x: +q.getAttribute('x') || 0, y: +q.getAttribute('y') || 0 });
      }
    }
    cells.set(id, { id, parent: el.getAttribute('parent'), value: wrap ? wrap.getAttribute('label') : el.getAttribute('value'), st: st.m, names: st.names, vertex: el.getAttribute('vertex') === '1', edge: el.getAttribute('edge') === '1', source: el.getAttribute('source'), target: el.getAttribute('target'), geo });
  }
  const origin = c => {
    let x = 0, y = 0, p = cells.get(c.parent);
    while (p && p.id !== '0' && p.id !== '1') { if (p.vertex && p.geo) { x += p.geo.x; y += p.geo.y; } p = cells.get(p.parent); }
    return { x, y };
  };
  const rects = new Map();
  const textOf = c => (c.st.html === '1' ? htmlToText(c.value) : c.value || '');
  const styleProps = (c, o, isText) => {
    const s = c.st, sw = s.strokeColor === 'none' ? 0 : Math.max(2, 2 * (Number(s.strokeWidth) || 1));
    const fc = normColor(s.fontColor, null);
    o.stroke = s.strokeColor === 'none' || !s.strokeColor || s.strokeColor === 'default' ? toInk(fc || '#000000') : toInk(normColor(s.strokeColor, '#000000'));
    o.sw = isText ? 0 : sw;
    o.fill = isText ? 'transparent' : (s.fillColor && s.fillColor !== 'default' ? normColor(s.fillColor, 'transparent') : 'transparent');
    if (s.dashed === '1') o.dash = s.dashPattern && /^1\s/.test(s.dashPattern) ? 'dotted' : 'dashed';
    if (s.opacity) o.opacity = clamp(Number(s.opacity) / 100, 0.05, 1);
    o.font = fontKey(s.fontFamily); o.fontSize = Number(s.fontSize) || 12;
    if (s.align) o.align = s.align === 'left' || s.align === 'right' ? s.align : 'center';
    if (s.rotation) o.angle = (Number(s.rotation) * Math.PI) / 180;
  };
  for (const c of cells.values()) {
    if (!c.vertex || !c.geo || c.id === '0' || c.id === '1') continue;
    const s = c.st, nm = c.names, og = origin(c), g = c.geo;
    let { w, h } = g; const x = g.x + og.x, y = g.y + og.y;
    const shapeStr = (s.shape || nm[0] || '').toLowerCase();
    let o, kind = 'rect';
    if (s.shape === 'image' || (s.image && !nm.includes('text'))) {
      const url = dioImage(s.image);
      if (url) { const aid = uid(); assets[aid] = url; o = mk('image', { asset: aid, sw: 0, x, y, w, h, angle: ((Number(s.rotation) || 0) * Math.PI) / 180 }); out.push(o); rects.set(c.id, { x, y, w, h, kind }); continue; }
      rep.skipped++; continue;
    }
    if (nm.includes('text') || s.shape === 'text') {
      o = mk('text'); styleProps(c, o, true); o.stroke = toInk(normColor(s.fontColor, '#000000'));
      o.text = textOf(c); if (!o.text) { rects.set(c.id, { x, y, w, h, kind }); continue; }
      o.align = s.align === 'center' || s.align === 'right' ? s.align : 'left';
      fitText(o); o.x = o.align === 'center' ? x + w / 2 - o.w / 2 : o.align === 'right' ? x + w - o.w : x; o.y = s.verticalAlign === 'middle' ? y + h / 2 - o.h / 2 : y;
      out.push(o); rects.set(c.id, { x, y, w, h, kind }); continue;
    }
    let type = 'rect', round = s.rounded === '1', angleAdd = 0, approx = false;
    if (/^(ellipse|doubleellipse)$/.test(shapeStr) || nm.includes('ellipse')) type = 'ellipse';
    else if (shapeStr === 'rhombus' || /decision|diamond|rhombus/.test(shapeStr) || nm.includes('rhombus')) type = 'diamond';
    else if (shapeStr === 'triangle' || nm.includes('triangle')) {
      type = 'triangle';
      const d = s.direction || 'east';
      if (d === 'north') angleAdd = 0; else if (d === 'east') { angleAdd = Math.PI / 2; [w, h] = [h, w]; } else if (d === 'south') angleAdd = Math.PI; else { angleAdd = -Math.PI / 2; [w, h] = [h, w]; }
    } else if (shapeStr === 'note' || s.tbType === 'sticky') type = 'sticky';
    else {
      if (/terminator|stadium|start/.test(shapeStr)) round = true;
      else if (s.shape && !/^(rectangle|mxgraph\.basic\.rect|process)$/.test(shapeStr)) approx = true;
      if (s.shape === 'swimlane' || nm.includes('swimlane')) approx = true;
    }
    if (approx) rep.approx++;
    kind = type === 'ellipse' ? 'ellipse' : type === 'diamond' ? 'diamond' : 'rect';
    o = mk(type, { x, y, w, h });
    styleProps(c, o, false);
    if (type === 'rect') o.round = round;
    if (type === 'sticky') { o.fill = normColor(s.fillColor, '#ffec99'); if (o.fill === 'transparent') o.fill = '#ffec99'; o.sw = 0; o.stroke = 'ink'; }
    if (angleAdd) { o.angle = (o.angle || 0) + angleAdd; o.x = x + g.w / 2 - w / 2; o.y = y + g.h / 2 - h / 2; }
    const label = textOf(c);
    if (label) {
      if ((s.verticalAlign === 'top' || s.verticalAlign === 'bottom') && h >= 50) {
        const t = mk('text', { stroke: o.stroke, font: o.font, fontSize: o.fontSize, align: o.align === 'left' || o.align === 'right' ? o.align : 'center', text: label });
        fitText(t);
        t.x = t.align === 'left' ? x + 8 : t.align === 'right' ? x + w - t.w - 8 : x + w / 2 - t.w / 2;
        t.y = s.verticalAlign === 'top' ? y + 6 : y + h - t.h - 6;
        out.push(o, t);
      } else { o.text = label; out.push(o); }
    } else out.push(o);
    rects.set(c.id, { x, y, w: g.w, h: g.h, kind });
  }
  const labelAt = (c, pts) => {
    const t = textOf(c); if (!t) return;
    const i = Math.floor((pts.length - 1) / 2), a = pts[i], b = pts[i + 1], mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
    const lb = textAt(t, mid[0], mid[1] - 12, Number(c.st.fontSize) || 12, 'center', { font: fontKey(c.st.fontFamily), stroke: toInk(normColor(c.st.fontColor, '#000000')) });
    out.push(lb);
  };
  for (const c of cells.values()) {
    if (!c.edge || !c.geo) continue;
    const s = c.st, og = origin(c), src = rects.get(c.source), tgt = rects.get(c.target);
    const wp = c.geo.pts.map(p => [p.x + og.x, p.y + og.y]);
    const sp = c.geo.sp && [c.geo.sp.x + og.x, c.geo.sp.y + og.y], tp = c.geo.tp && [c.geo.tp.x + og.x, c.geo.tp.y + og.y];
    const center = r => [r.x + r.w / 2, r.y + r.h / 2];
    const side = (r, fx, fy) => (fx != null && fy != null ? [r.x + Number(fx) * r.w, r.y + Number(fy) * r.h] : null);
    let a = (src && side(src, s.exitX, s.exitY)) || sp, z = (tgt && side(tgt, s.entryX, s.entryY)) || tp;
    const toward = (fallbackR, list, idx) => (list.length ? list[idx] : fallbackR ? center(fallbackR) : null);
    if (!a && src) { const t = wp.length ? wp[0] : z || (tgt && center(tgt)); a = t ? boundaryPoint(src, t[0], t[1]) : center(src); }
    if (!z && tgt) { const t = wp.length ? wp[wp.length - 1] : a || (src && center(src)); z = t ? boundaryPoint(tgt, t[0], t[1]) : center(tgt); }
    void toward;
    if (!a || !z) { rep.skipped++; continue; }
    const pts = [a, ...wp, z];
    const endA = s.endArrow === undefined ? 'classic' : s.endArrow, startA = s.startArrow || 'none';
    out.push(...polyline(pts, { arrowEnd: endA !== 'none', arrowStart: startA !== 'none', dash: s.dashed === '1' ? 'dashed' : 'solid', sw: Math.max(2, 2 * (Number(s.strokeWidth) || 1)), stroke: toInk(normColor(s.strokeColor, '#000000')), opacity: s.opacity ? Number(s.opacity) / 100 : 1 }));
    labelAt(c, pts);
  }
  const bgAttr = model.getAttribute('background');
  const dg = diagram || xml.querySelector('diagram');
  return { objects: out, assets, rep, name: (dg && dg.getAttribute('name')) || baseName, bg: bgAttr && /^#/.test(bgAttr) ? nearestBg(bgAttr) : undefined };
}

/* =====================================================================
   Excalidraw (.excalidraw)
   ===================================================================== */
const XFONT = { sans: 2, hand: 1, mono: 3, serif: 2 };
const XFONT_BACK = { 1: 'hand', 2: 'sans', 3: 'mono', 4: 'sans', 5: 'hand', 6: 'sans', 7: 'hand', 8: 'mono', 9: 'serif' };
function exportExcalidrawObj(objs) {
  const els = [], files = {}, now = Date.now(), rnd = () => Math.floor(Math.random() * 2 ** 31);
  const swx = o => Math.max(1, Math.round(o.sw / 1.6));
  const base = (o, type, extra) => ({
    id: o.id, type, x: o.x, y: o.y, width: o.w, height: o.h, angle: o.angle || 0,
    strokeColor: o.sw > 0 || type === 'text' ? col(o.stroke) : 'transparent', backgroundColor: o.fill && o.fill !== 'transparent' ? col(o.fill) : 'transparent',
    fillStyle: 'solid', strokeWidth: swx(o), strokeStyle: o.dash || 'solid', roughness: 0, opacity: Math.round((o.opacity ?? 1) * 100),
    groupIds: o.gid ? [o.gid] : [], frameId: null, roundness: null, seed: rnd(), version: 1, versionNonce: rnd(), isDeleted: false,
    boundElements: null, updated: now, link: null, locked: !!o.locked, ...extra,
  });
  const textEl = (o, container, standalone) => {
    const li = labelInfo(o); if (!o.text) return null;
    mctx.font = fontStr(o);
    const lines = standalone ? String(o.text).split('\n') : li.lines;
    const w = Math.max(...lines.map(l => mctx.measureText(l).width), 4), h = lines.length * li.lh;
    const bc = toWorld(o, li.box.x + li.box.w / 2, li.box.y + li.box.h / 2);
    const t = base({ ...o, id: uid(), sw: 1, fill: 'transparent', gid: container ? o.gid : o.gid }, 'text', {
      x: standalone ? o.x : bc.x - w / 2, y: standalone ? o.y : bc.y - h / 2, width: standalone ? o.w : w, height: h, strokeColor: o.type === 'sticky' ? '#1e1e1e' : col(o.stroke),
      text: lines.join('\n'), originalText: o.text, fontSize: o.fontSize, fontFamily: XFONT[o.font] || 2, textAlign: o.align || 'left', verticalAlign: standalone ? 'top' : 'middle',
      containerId: container ? container.id : null, autoResize: true, lineHeight: 1.3, angle: o.angle || 0, strokeWidth: 1, roundness: null,
    });
    return t;
  };
  for (const o of objs) {
    switch (o.type) {
      case 'rect': case 'ellipse': case 'diamond': case 'sticky': {
        const type = o.type === 'rect' || o.type === 'sticky' ? 'rectangle' : o.type;
        const el = base(o, type, { roundness: o.type === 'rect' && o.round ? { type: 3 } : null });
        if (o.type === 'sticky') { el.strokeColor = 'transparent'; el.backgroundColor = o.fill; }
        const t = textEl(o, el, false);
        if (t) el.boundElements = [{ id: t.id, type: 'text' }];
        els.push(el); if (t) els.push(t); break;
      }
      case 'triangle': {
        els.push(base(o, 'line', { points: [[o.w / 2, 0], [o.w, o.h], [0, o.h], [o.w / 2, 0]], polygon: true, lastCommittedPoint: null, startBinding: null, endBinding: null, startArrowhead: null, endArrowhead: null }));
        const t = textEl(o, null, false); if (t) els.push(t); break;
      }
      case 'text': { const t = textEl(o, null, true); if (t) { t.id = o.id; els.push(t); } break; }
      case 'line': {
        const p = absPts(o), dx = p[1][0] - p[0][0], dy = p[1][1] - p[0][1], arrow = o.arrowStart || o.arrowEnd;
        els.push(base(o, arrow ? 'arrow' : 'line', {
          x: p[0][0], y: p[0][1], width: Math.abs(dx), height: Math.abs(dy), angle: 0, points: [[0, 0], [dx, dy]], lastCommittedPoint: null, startBinding: null, endBinding: null,
          startArrowhead: o.arrowStart ? 'arrow' : null, endArrowhead: o.arrowEnd ? 'arrow' : null, elbowed: false, roundness: arrow ? { type: 2 } : null,
        })); break;
      }
      case 'pen': {
        const p = absPts(o), x0 = p[0][0], y0 = p[0][1], xs = p.map(q => q[0]), ys = p.map(q => q[1]);
        els.push(base(o, 'freedraw', { x: x0, y: y0, width: Math.max(...xs) - Math.min(...xs), height: Math.max(...ys) - Math.min(...ys), angle: 0, points: p.map(q => [q[0] - x0, q[1] - y0]), pressures: [], simulatePressure: true, lastCommittedPoint: null })); break;
      }
      case 'image': {
        const url = S.assets[o.asset]; if (!url) break;
        const fid = o.asset; files[fid] = { mimeType: dataMime(url), id: fid, dataURL: url, created: now, lastRetrieved: now };
        els.push(base(o, 'image', { fileId: fid, status: 'saved', scale: [1, 1], crop: null })); break;
      }
    }
  }
  return { type: 'excalidraw', version: 2, source: 'tableau-blanc', elements: els, appState: { gridSize: null, viewBackgroundColor: bgInfo().c, theme: bgInfo().dark ? 'dark' : 'light' }, files };
}

function importExcalidraw(json, baseName) {
  const els = (json.elements || []).filter(e => !e.isDeleted), files = json.files || {};
  const out = [], assets = {}, rep = { approx: 0, skipped: 0 };
  const byIdX = new Map(els.map(e => [e.id, e])), contText = new Map();
  for (const e of els) if (e.type === 'text' && e.containerId) { const c = byIdX.get(e.containerId); if (c && ['rectangle', 'ellipse', 'diamond'].includes(c.type)) contText.set(c.id, e); }
  const common = (e, o, isText) => {
    const tr = e.strokeColor === 'transparent';
    o.stroke = tr ? 'ink' : toInk(normColor(e.strokeColor, 'ink'));
    o.sw = isText || tr ? 0 : Math.max(1.5, Math.round(e.strokeWidth * 3) / 2);
    o.fill = isText || e.backgroundColor === 'transparent' || !e.backgroundColor ? 'transparent' : normColor(e.backgroundColor, 'transparent');
    o.dash = e.strokeStyle === 'dashed' || e.strokeStyle === 'dotted' ? e.strokeStyle : 'solid';
    o.opacity = clamp((e.opacity ?? 100) / 100, 0.05, 1);
    if (e.angle) o.angle = e.angle;
    if (e.groupIds && e.groupIds.length) o.gid = e.groupIds[e.groupIds.length - 1];
    if (e.locked) o.locked = true;
    return o;
  };
  const stampText = (o, t) => { o.text = t.originalText || t.text || ''; o.font = XFONT_BACK[t.fontFamily] || 'sans'; o.fontSize = t.fontSize || 20; o.align = t.textAlign || 'center'; if (o.type === 'rect' || o.type === 'ellipse' || o.type === 'diamond') o.stroke = o.stroke; };
  for (const e of els) {
    switch (e.type) {
      case 'rectangle': case 'ellipse': case 'diamond': {
        const type = e.type === 'rectangle' ? 'rect' : e.type;
        const o = common(e, mk(type, { x: e.x, y: e.y, w: e.width, h: e.height }), false);
        if (type === 'rect') o.round = !!e.roundness;
        const t = contText.get(e.id); if (t) { stampText(o, t); if (t.strokeColor && o.sw === 0) o.stroke = toInk(normColor(t.strokeColor, 'ink')); }
        out.push(o); break;
      }
      case 'text': {
        if (e.containerId && contText.get(e.containerId) === e) break;
        const o = common(e, mk('text', { x: e.x, y: e.y }), true);
        o.stroke = toInk(normColor(e.strokeColor, 'ink')); o.sw = 0; o.text = e.text || ''; o.font = XFONT_BACK[e.fontFamily] || 'sans'; o.fontSize = e.fontSize || 20; o.align = e.textAlign || 'left';
        fitText(o); out.push(o); break;
      }
      case 'line': case 'arrow': {
        const pts = (e.points || []).map(p => [e.x + p[0], e.y + p[1]]);
        if (pts.length < 2) { rep.skipped++; break; }
        const closed = pts.length === 4 && Math.hypot(pts[0][0] - pts[3][0], pts[0][1] - pts[3][1]) < 2;
        if (e.type === 'line' && closed) {
          const xs = pts.map(p => p[0]), ys = pts.map(p => p[1]), x0 = Math.min(...xs), y0 = Math.min(...ys);
          const o = common({ ...e, angle: 0 }, mk('triangle', { x: x0, y: y0, w: Math.max(...xs) - x0, h: Math.max(...ys) - y0 }), false);
          rep.approx++; out.push(o); break;
        }
        const st = common(e, mk('line'), false);
        out.push(...polyline(pts, { arrowStart: !!e.startArrowhead, arrowEnd: !!e.endArrowhead, dash: st.dash, sw: st.sw || 2, stroke: st.stroke, opacity: st.opacity }));
        break;
      }
      case 'freedraw': {
        let pts = (e.points || []).map(p => [e.x + p[0], e.y + p[1]]);
        if (!pts.length) break; if (pts.length === 1) pts.push([...pts[0]]);
        const o = common(e, mk('pen', { dash: 'solid' }), false); o.angle = 0; o.sw = Math.max(1.5, Math.round(e.strokeWidth * 3) / 2);
        out.push(setAbsPoints(o, pts)); break;
      }
      case 'image': {
        const f = files[e.fileId]; if (!f || !f.dataURL) { rep.skipped++; break; }
        const aid = uid(); assets[aid] = f.dataURL;
        out.push(mk('image', { asset: aid, sw: 0, x: e.x, y: e.y, w: e.width, h: e.height, angle: e.angle || 0, opacity: clamp((e.opacity ?? 100) / 100, 0.05, 1) })); break;
      }
      case 'frame': case 'magicframe': {
        out.push(mk('rect', { x: e.x, y: e.y, w: e.width, h: e.height, dash: 'dashed', sw: 2 }));
        if (e.name) out.push(mk('text', { text: e.name, x: e.x, y: e.y - 26, fontSize: 16 }));
        rep.approx++; break;
      }
      default: rep.skipped++;
    }
  }
  for (const o of out) if (o.type === 'text') fitText(o);
  const bg = json.appState && json.appState.viewBackgroundColor;
  return { objects: out, assets, rep, name: baseName, bg: bg ? nearestBg(bg) : undefined };
}

/* =====================================================================
   JSON Canvas (.canvas, Obsidian)
   ===================================================================== */
const JC_COLORS = { 1: '#e03131', 2: '#f08c00', 3: '#f2c600', 4: '#2f9e44', 5: '#0c8599', 6: '#7048e8' };
function exportJsonCanvasObj(objs) {
  const nodes = [], edges = [], boxes = [], rep = { dropped: 0 };
  for (const o of objs) {
    if (!['rect', 'ellipse', 'diamond', 'triangle', 'sticky', 'text'].includes(o.type)) continue;
    const b = bboxOf(o), n = { id: o.id, type: 'text', text: o.text || '', x: Math.round(b.x), y: Math.round(b.y), width: Math.max(60, Math.round(b.w)), height: Math.max(40, Math.round(b.h)) };
    const c = o.type === 'sticky' ? o.fill : o.fill !== 'transparent' ? o.fill : o.stroke !== 'ink' ? col(o.stroke) : null;
    if (c && /^#/.test(c)) n.color = c;
    nodes.push(n); boxes.push({ id: o.id, ...b });
  }
  const near = p => {
    let best = null, bd = 18;
    for (const b of boxes) {
      const dx = Math.max(b.x - p[0], 0, p[0] - (b.x + b.w)), dy = Math.max(b.y - p[1], 0, p[1] - (b.y + b.h)), d = Math.hypot(dx, dy);
      if (d <= bd) { bd = d; best = b; }
    }
    return best;
  };
  const sideOf = (b, p) => { const dx = (p[0] - (b.x + b.w / 2)) / (b.w || 1), dy = (p[1] - (b.y + b.h / 2)) / (b.h || 1); return Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? 'right' : 'left') : dy > 0 ? 'bottom' : 'top'; };
  for (const o of objs) {
    if (o.type !== 'line') { if (o.type === 'pen' || o.type === 'image') rep.dropped++; continue; }
    const p = absPts(o), a = near(p[0]), z = near(p[1]);
    if (!a || !z || a === z) { rep.dropped++; continue; }
    edges.push({ id: o.id, fromNode: a.id, toNode: z.id, fromSide: sideOf(a, p[0]), toSide: sideOf(z, p[1]), fromEnd: o.arrowStart ? 'arrow' : 'none', toEnd: o.arrowEnd ? 'arrow' : 'none' });
  }
  return { json: { nodes, edges }, rep };
}
function importJsonCanvas(json, baseName) {
  const out = [], rep = { approx: 0, skipped: 0 }, rects = new Map();
  const colorOf = c => (c ? JC_COLORS[c] || normColor(c, null) : null);
  for (const n of json.nodes || []) {
    const c = colorOf(n.color), base = { x: n.x, y: n.y, w: n.width, h: n.height };
    if (n.type === 'group') {
      out.push(mk('rect', { ...base, dash: 'dashed', sw: 2, stroke: c || 'ink', round: true }));
      if (n.label) out.push(mk('text', { text: n.label, x: n.x + 8, y: n.y - 26, fontSize: 16, stroke: c || 'ink' }));
    } else {
      const text = n.type === 'text' ? n.text || '' : n.type === 'file' ? '📄 ' + (n.file || '') : n.type === 'link' ? n.url || '' : '';
      if (n.type !== 'text') rep.approx++;
      out.push(mk('rect', { ...base, text, round: true, fontSize: 16, stroke: c || 'ink', align: 'left' }));
    }
    rects.set(n.id, { ...base, kind: 'rect' });
  }
  const sidePt = (r, side) => ({ top: [r.x + r.w / 2, r.y], bottom: [r.x + r.w / 2, r.y + r.h], left: [r.x, r.y + r.h / 2], right: [r.x + r.w, r.y + r.h / 2] }[side]);
  for (const e of json.edges || []) {
    const a = rects.get(e.fromNode), z = rects.get(e.toNode); if (!a || !z) { rep.skipped++; continue; }
    const ca = [a.x + a.w / 2, a.y + a.h / 2], cz = [z.x + z.w / 2, z.y + z.h / 2];
    const p0 = sidePt(a, e.fromSide) || boundaryPoint(a, cz[0], cz[1]), p1 = sidePt(z, e.toSide) || boundaryPoint(z, ca[0], ca[1]);
    out.push(...polyline([p0, p1], { arrowEnd: (e.toEnd || 'arrow') !== 'none', arrowStart: (e.fromEnd || 'none') !== 'none', stroke: colorOf(e.color) || 'ink' }));
    if (e.label) out.push(textAt(e.label, (p0[0] + p1[0]) / 2, (p0[1] + p1[1]) / 2 - 12));
  }
  return { objects: out, assets: {}, rep, name: baseName };
}

/* =====================================================================
   Mermaid : flowchart / graph, stateDiagram, sequenceDiagram
   ===================================================================== */
const MM_FS = 18;
const MM_OPEN = [['(((', ')))', 'circle'], ['((', '))', 'circle'], ['([', '])', 'stadium'], ['[(', ')]', 'cyl'], ['{{', '}}', 'hex'], ['[[', ']]', 'sub'], ['[/', '/]', 'para'], ['[\\', '\\]', 'para'], ['[/', '\\]', 'trap'], ['[\\', '/]', 'trap'], ['[', ']', 'rect'], ['(', ')', 'round'], ['{', '}', 'diamond'], ['>', ']', 'flag']];
const mmText = s => String(s == null ? '' : s).replace(/<br\s*\/?>/gi, '\n').replace(/^"|"$/g, '').replace(/&quot;/g, '"').replace(/<[^>]+>/g, '');

function parseNodeTok(s, i) {
  const m = /^[\wÀ-￿]+/.exec(s.slice(i)); if (!m) return null;
  const id = m[0]; let j = i + id.length, label = null, shape = null;
  for (const [o, c, sh] of MM_OPEN) {
    if (!s.startsWith(o, j)) continue;
    const k = j + o.length; let text, e;
    if (s[k] === '"') { const q = s.indexOf('"', k + 1); if (q < 0) break; text = s.slice(k + 1, q); e = s.indexOf(c, q + 1); } else { e = s.indexOf(c, k); text = e < 0 ? '' : s.slice(k, e); }
    if (e < 0) break;
    j = e + c.length; label = text; shape = sh; break;
  }
  let cls = null; const cm = /^:::([\w-]+)/.exec(s.slice(j)); if (cm) { cls = cm[1]; j += cm[0].length; }
  return { id, label, shape, cls, end: j };
}
function readLink(s) {
  let m;
  if ((m = /^(<?)(--|==)\s+(?!-)([^<>]*?)\s+(-{2,}|={2,})([>xo]?)/.exec(s))) return { len: m[0].length, p: { label: m[3], start: !!m[1], end: !!m[5], thick: m[2] === '==', dash: 'solid' } };
  if ((m = /^(<?)-\.\s+([^<>]*?)\s+\.-+([>xo]?)/.exec(s))) return { len: m[0].length, p: { label: m[2], start: !!m[1], end: !!m[3], dash: 'dashed' } };
  if ((m = /^(<?)-\.+-([>xo]?)(?:\|([^|]*)\|)?/.exec(s))) return { len: m[0].length, p: { label: m[3] || '', start: !!m[1], end: !!m[2], dash: 'dashed' } };
  if ((m = /^~~~/.exec(s))) return { len: m[0].length, p: { invisible: true } };
  if ((m = /^(<?)(-{2,}|={2,})([>xo]?)(?:\|([^|]*)\|)?/.exec(s))) return { len: m[0].length, p: { label: m[4] || '', start: !!m[1], end: !!m[3], thick: m[2][0] === '=', dash: 'solid' } };
  return null;
}
function parseStyleProps(str) {
  const o = {};
  for (const kv of String(str).split(',')) { const i = kv.indexOf(':'); if (i > 0) o[kv.slice(0, i).trim()] = kv.slice(i + 1).trim(); }
  return o;
}
function newFlow() { return { nodes: new Map(), edges: [], subs: [], stack: [], classes: {}, dir: 'TD' }; }
function flowNode(M, t) {
  let n = M.nodes.get(t.id);
  if (!n) { n = { id: t.id, label: t.id, shape: 'rect', order: M.nodes.size, props: {} }; M.nodes.set(t.id, n); if (M.stack.length) { const sub = M.stack[M.stack.length - 1]; n.sub = sub; sub.members.push(n.id); } }
  if (t.label != null) { n.label = mmText(t.label); n.shape = t.shape; }
  if (t.cls) Object.assign(n.props, M.classes[t.cls] || {});
  return n;
}
function parseFlowLine(s, M) {
  let i = 0;
  const skip = () => { while (s[i] === ' ') i++; };
  const readGroup = () => {
    const g = [];
    for (;;) {
      skip(); const t = parseNodeTok(s, i); if (!t) return g.length ? g : null;
      i = t.end; g.push(flowNode(M, t)); skip();
      if (s[i] === '&') { i++; continue; }
      return g;
    }
  };
  let prev = readGroup(); if (!prev) return false;
  for (;;) {
    skip(); if (i >= s.length) break;
    const l = readLink(s.slice(i)); if (!l) break;
    i += l.len; const next = readGroup(); if (!next) break;
    for (const a of prev) for (const b of next) M.edges.push({ from: a.id, to: b.id, ...l.p });
    prev = next;
  }
  return true;
}
function parseFlowchart(lines, M) {
  for (const raw of lines) for (const stmt of raw.split(';')) {
    const s = stmt.trim(); if (!s) continue; let m;
    if ((m = /^subgraph\s+(.+)$/i.exec(s))) {
      const rest = m[1].trim(), mm = /^([\wÀ-￿]+)\s*\[(.*)\]\s*$/.exec(rest);
      const sub = { id: mm ? mm[1] : rest, title: mmText(mm ? mm[2] : rest), members: [], parent: M.stack[M.stack.length - 1] || null };
      M.subs.push(sub); M.stack.push(sub);
    } else if (/^end$/i.test(s)) M.stack.pop();
    else if (/^direction\s/i.test(s) || /^(linkStyle|click|accTitle|accDescr|title)\b/.test(s)) continue;
    else if ((m = /^classDef\s+([\w,-]+)\s+(.+)$/.exec(s))) for (const c of m[1].split(',')) M.classes[c] = parseStyleProps(m[2]);
    else if ((m = /^class\s+([^\s]+)\s+([\w-]+)$/.exec(s))) for (const id of m[1].split(',')) { const n = M.nodes.get(id); if (n) Object.assign(n.props, M.classes[m[2]] || {}); }
    else if ((m = /^style\s+([\wÀ-￿]+)\s+(.+)$/.exec(s))) { const n = M.nodes.get(m[1]); if (n) Object.assign(n.props, parseStyleProps(m[2])); }
    else parseFlowLine(s, M);
  }
}
function parseState(lines, M) {
  const ref = (id, asSource) => {
    if (id === '[*]') {
      const sub = M.stack[M.stack.length - 1], key = (asSource ? '__start' : '__end') + (sub ? sub.id : '');
      return flowNode(M, { id: key, label: '', shape: asSource ? 'start' : 'end' }).id;
    }
    return flowNode(M, { id, label: null }).id;
  };
  for (const s of lines) {
    let m;
    if ((m = /^direction\s+(TB|TD|LR|RL|BT)/i.exec(s))) M.dir = m[1].toUpperCase();
    else if ((m = /^state\s+"(.+)"\s+as\s+(\w+)/.exec(s))) flowNode(M, { id: m[2], label: m[1], shape: 'round' });
    else if ((m = /^state\s+(\w+)\s*<<(choice|fork|join)>>/.exec(s))) flowNode(M, { id: m[1], label: '', shape: m[2] === 'choice' ? 'diamond' : 'bar' });
    else if ((m = /^state\s+(?:"([^"]+)"\s+as\s+)?(\w+)\s*\{$/.exec(s))) { const sub = { id: m[2], title: m[1] || m[2], members: [], parent: M.stack[M.stack.length - 1] || null }; M.subs.push(sub); M.stack.push(sub); }
    else if (s === '}') M.stack.pop();
    else if ((m = /^(\[\*\]|\w+)\s*-->\s*(\[\*\]|\w+)\s*(?::\s*(.*))?$/.exec(s))) { const a = ref(m[1], true), b = ref(m[2], false); M.edges.push({ from: a, to: b, label: mmText(m[3] || ''), end: true, dash: 'solid' }); }
    else if ((m = /^(\w+)\s*:\s*(.+)$/.exec(s))) { const n = flowNode(M, { id: m[1], label: mmText(m[2]), shape: 'round' }); n.shape = 'round'; }
  }
}
function sizeNode(n) {
  const sh = n.shape, o = { text: n.label, font: 'sans', fontSize: MM_FS, align: 'center' };
  if (sh === 'start') return { type: 'ellipse', w: 26, h: 26, o: { ...o, text: '' }, fillInk: true };
  if (sh === 'end') return { type: 'ellipse', w: 28, h: 28, o: { ...o, text: '' }, ring: true };
  if (sh === 'bar') return { type: 'rect', w: 90, h: 10, o: { ...o, text: '' }, fillInk: true };
  const type = sh === 'circle' ? 'ellipse' : sh === 'diamond' ? 'diamond' : 'rect';
  mctx.font = `${MM_FS}px ${FONTS.sans}`;
  const tw = Math.max(0, ...String(n.label).split('\n').map(l => mctx.measureText(l).width));
  let w = type === 'ellipse' ? clamp(tw / 0.7 + 32, 90, 300) : type === 'diamond' ? clamp(tw / 0.5 + 24, 130, 340) : clamp(tw + 40, 90, 300);
  let h = type === 'ellipse' ? 60 : type === 'diamond' ? 84 : 52;
  const t = { type, w, h, ...o };
  for (let i = 0; i < 6; i++) {
    const li = labelInfo(t); if (li.total <= li.box.h) break;
    const ratio = type === 'rect' ? 1 : type === 'ellipse' ? 0.7 : 0.5;
    t.h += (li.total - li.box.h) / ratio + 4;
  }
  return { type, w: t.w, h: t.h, o: { ...o }, round: sh === 'round' || sh === 'stadium' };
}
function layoutLayered(nodes, edges, dir) {
  const horiz = dir === 'LR' || dir === 'RL', items = new Map();
  for (const n of nodes) items.set(n.id, { id: n.id, a: horiz ? n.w : n.h, b: horiz ? n.h : n.w, rank: 0 });
  const E = [];
  for (const e of edges) if (e.from !== e.to && items.has(e.from) && items.has(e.to)) E.push({ from: e.from, to: e.to, ref: e });
  const adj = new Map(nodes.map(n => [n.id, []])); E.forEach((e, i) => adj.get(e.from).push(i));
  const state = new Map(), rev = new Set();
  const dfs = u => { state.set(u, 1); for (const i of adj.get(u)) { const v = E[i].to, s = state.get(v) || 0; if (s === 1) rev.add(i); else if (s === 0) dfs(v); } state.set(u, 2); };
  for (const n of nodes) if (!state.get(n.id)) dfs(n.id);
  const O = E.map((e, i) => (rev.has(i) ? { u: e.to, v: e.from, i, rev: true } : { u: e.from, v: e.to, i, rev: false }));
  const preds = new Map(nodes.map(n => [n.id, []])); O.forEach(e => preds.get(e.v).push(e.u));
  const rk = new Map();
  const rank = u => { if (rk.has(u)) return rk.get(u); rk.set(u, 0); let r = 0; for (const p of preds.get(u)) r = Math.max(r, rank(p) + 1); rk.set(u, r); return r; };
  nodes.forEach(n => (items.get(n.id).rank = rank(n.id)));
  let dn = 0;
  for (const e of O) {
    const ru = items.get(e.u).rank, rv = items.get(e.v).rank; e.chain = [e.u];
    for (let r = ru + 1; r < rv; r++) { const id = '__d' + dn++; items.set(id, { id, a: 0, b: 12, rank: r, dummy: true }); e.chain.push(id); }
    e.chain.push(e.v);
  }
  const R = Math.max(0, ...[...items.values()].map(n => n.rank)), layers = Array.from({ length: R + 1 }, () => []);
  for (const n of items.values()) layers[n.rank].push(n.id);
  const up = new Map(), down = new Map(); items.forEach((_, id) => { up.set(id, []); down.set(id, []); });
  for (const e of O) for (let k = 0; k < e.chain.length - 1; k++) { down.get(e.chain[k]).push(e.chain[k + 1]); up.get(e.chain[k + 1]).push(e.chain[k]); }
  const pos = new Map(), reindex = l => l.forEach((id, i) => pos.set(id, i));
  layers.forEach(reindex);
  const bary = (id, nb) => { const a = nb.get(id); return a.length ? a.reduce((s, x) => s + pos.get(x), 0) / a.length : pos.get(id); };
  const sortLayer = (l, nb) => { const key = new Map(l.map(id => [id, bary(id, nb)])); l.sort((p, q) => key.get(p) - key.get(q)); reindex(l); };
  for (let it = 0; it < 8; it++) { if (it % 2 === 0) for (let r = 1; r <= R; r++) sortLayer(layers[r], up); else for (let r = R - 1; r >= 0; r--) sortLayer(layers[r], down); }
  const sep = 48, cx = new Map();
  for (const l of layers) {
    let x = 0; for (const id of l) { const w = items.get(id).b; x += w / 2; cx.set(id, x); x += w / 2 + sep; }
    const shift = (x - sep) / 2; for (const id of l) cx.set(id, cx.get(id) - shift);
  }
  const settle = (l, desire) => {
    const n = l.length, d = l.map(desire), w = l.map(id => items.get(id).b), x1 = [], x2 = [];
    for (let i = 0; i < n; i++) x1[i] = i ? Math.max(d[i], x1[i - 1] + (w[i - 1] + w[i]) / 2 + sep) : d[i];
    for (let i = n - 1; i >= 0; i--) x2[i] = i < n - 1 ? Math.min(d[i], x2[i + 1] - (w[i] + w[i + 1]) / 2 - sep) : d[i];
    l.forEach((id, i) => cx.set(id, (x1[i] + x2[i]) / 2));
  };
  const avg = (nb, id) => { const a = nb.get(id); return a.length ? a.reduce((s, x) => s + cx.get(x), 0) / a.length : cx.get(id); };
  for (let it = 0; it < 6; it++) {
    for (let r = 1; r <= R; r++) settle(layers[r], id => avg(up, id));
    for (let r = R - 1; r >= 0; r--) settle(layers[r], id => avg(down, id));
  }
  const A = []; let prevM = 0;
  layers.forEach((l, r) => { const m = Math.max(0, ...l.map(id => items.get(id).a)); A[r] = r === 0 ? m / 2 : A[r - 1] + prevM / 2 + 84 + m / 2; prevM = m; });
  const place = id => {
    const a = A[items.get(id).rank], b = cx.get(id);
    return dir === 'LR' ? [a, b] : dir === 'RL' ? [-a, b] : dir === 'BT' ? [b, -a] : [b, a];
  };
  const centers = new Map(nodes.map(n => [n.id, place(n.id)]));
  const routes = E.map((e, i) => { const oe = O[i], pts = oe.chain.map(place); return { ref: e.ref, pts: oe.rev ? pts.reverse() : pts }; });
  return { centers, routes };
}
function mermaidFlow(M) {
  const nodes = [...M.nodes.values()].sort((a, b) => a.order - b.order), sized = new Map();
  for (const n of nodes) sized.set(n.id, sizeNode(n));
  const { centers, routes } = layoutLayered(nodes.map(n => ({ id: n.id, w: sized.get(n.id).w, h: sized.get(n.id).h })), M.edges, M.dir);
  const out = [], rects = new Map(), edgeObjs = [];
  for (const n of nodes) {
    const s = sized.get(n.id), c = centers.get(n.id), p = n.props;
    const o = mk(s.type, { x: c[0] - s.w / 2, y: c[1] - s.h / 2, w: s.w, h: s.h, ...s.o, round: !!s.round });
    if (s.type !== 'rect') delete o.round;
    if (s.fillInk) { o.fill = 'ink'; o.sw = 2; } if (s.ring) o.sw = 5;
    if (p.fill) o.fill = normColor(p.fill, 'transparent');
    if (p.stroke) o.stroke = toInk(normColor(p.stroke, 'ink'));
    else if (p.color) o.stroke = toInk(normColor(p.color, 'ink'));
    if (p['stroke-width']) o.sw = clamp(parseFloat(p['stroke-width']) * 1.5, 1.5, 10);
    if (/dash/.test(p['stroke-dasharray'] || '')) o.dash = 'dashed'; else if (p['stroke-dasharray']) o.dash = 'dashed';
    out.push(o); rects.set(n.id, { x: o.x, y: o.y, w: o.w, h: o.h, kind: s.type === 'ellipse' ? 'ellipse' : s.type === 'diamond' ? 'diamond' : 'rect' });
  }
  // sous-graphes : rectangle englobant, placé sous les nœuds
  const subObjs = [], bounds = new Map();
  const boxOf = sub => {
    if (bounds.has(sub.id)) return bounds.get(sub.id);
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const id of sub.members) { const r = rects.get(id); if (r) { x0 = Math.min(x0, r.x); y0 = Math.min(y0, r.y); x1 = Math.max(x1, r.x + r.w); y1 = Math.max(y1, r.y + r.h); } }
    for (const c of M.subs) if (c.parent === sub) { const b = boxOf(c); if (b) { x0 = Math.min(x0, b.x); y0 = Math.min(y0, b.y); x1 = Math.max(x1, b.x + b.w); y1 = Math.max(y1, b.y + b.h); } }
    const b = x0 === Infinity ? null : { x: x0 - 28, y: y0 - 46, w: x1 - x0 + 56, h: y1 - y0 + 74 };
    bounds.set(sub.id, b); return b;
  };
  const depth = s => (s.parent ? 1 + depth(s.parent) : 0);
  for (const sub of [...M.subs].sort((a, b) => depth(a) - depth(b))) {
    const b = boxOf(sub); if (!b) continue;
    subObjs.push(mk('rect', { x: b.x, y: b.y, w: b.w, h: b.h, dash: 'dashed', sw: 2, round: true, opacity: 0.8 }));
    const t = mk('text', { text: sub.title, fontSize: 16, x: b.x + 12, y: b.y + 8 }); fitText(t); subObjs.push(t);
  }
  // arêtes
  const pairCount = new Map(), pairIdx = new Map();
  for (const r of routes) { const k = [r.ref.from, r.ref.to].sort().join('|'); pairCount.set(k, (pairCount.get(k) || 0) + 1); }
  for (const r of routes) {
    const e = r.ref; if (e.invisible) continue;
    let pts = r.pts.map(p => [...p]);
    const rs = rects.get(e.from), rt = rects.get(e.to);
    pts[0] = boundaryPoint(rs, pts[1][0], pts[1][1]); pts[pts.length - 1] = boundaryPoint(rt, pts[pts.length - 2][0], pts[pts.length - 2][1]);
    const k = [e.from, e.to].sort().join('|'), n = pairCount.get(k), idx = pairIdx.get(k) || 0; pairIdx.set(k, idx + 1);
    if (n > 1 && pts.length === 2) {
      const dx = pts[1][0] - pts[0][0], dy = pts[1][1] - pts[0][1], l = Math.hypot(dx, dy) || 1, off = (idx - (n - 1) / 2) * 18;
      const ox = (-dy / l) * off, oy = (dx / l) * off; pts = pts.map(p => [p[0] + ox, p[1] + oy]);
    }
    edgeObjs.push(...polyline(pts, { arrowEnd: e.end !== false && (e.end || e.end === undefined), arrowStart: !!e.start, dash: e.dash || 'solid', sw: e.thick ? 5 : 2.5 }));
    if (e.label) {
      const i = Math.floor((pts.length - 1) / 2), a = pts[i], b = pts[i + 1], mid = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
      const t = mk('text', { text: e.label, fontSize: 15 }); fitText(t);
      if (Math.abs(b[1] - a[1]) > Math.abs(b[0] - a[0])) { t.x = mid[0] + 8; t.y = mid[1] - t.h / 2; } else { t.x = mid[0] - t.w / 2; t.y = mid[1] - t.h - 4; }
      edgeObjs.push(t);
    }
  }
  for (const e of M.edges) {
    if (e.from !== e.to || !rects.has(e.from)) continue;
    const r = rects.get(e.from), x = r.x + r.w, y0 = r.y + r.h / 2 - r.h / 4, y1 = r.y + r.h / 2 + r.h / 4;
    edgeObjs.push(...polyline([[x, y0], [x + 34, y0], [x + 34, y1], [x, y1]], { arrowEnd: true, sw: 2.5 }));
    if (e.label) edgeObjs.push(textAt(e.label, x + 34 + 6, (y0 + y1) / 2, 15, 'left', {}));
  }
  return [...subObjs, ...out, ...edgeObjs];
}
function mermaidSequence(lines) {
  const parts = new Map(), order = [], rows = [];
  const part = (id, label) => { if (!parts.has(id)) { parts.set(id, { id, label: label || id }); order.push(id); } else if (label) parts.get(id).label = label; return parts.get(id); };
  let auto = false, count = 0, row = 0; const frames = [], stack = [], marks = [];
  for (const s of lines) {
    let m;
    if ((m = /^(?:participant|actor)\s+(\S+)(?:\s+as\s+(.+))?$/i.exec(s))) part(m[1], m[2] && mmText(m[2]));
    else if (/^autonumber/i.test(s)) auto = true;
    else if ((m = /^(loop|alt|opt|par|critical|break|rect)\b\s*(.*)$/i.exec(s))) { stack.push({ kind: m[1], label: m[2], r0: row }); row += 0.6; }
    else if ((m = /^(else|and|option)\b\s*(.*)$/i.exec(s))) { marks.push({ r: row, label: `[${m[2] || m[1]}]` }); row += 0.6; }
    else if (/^end$/i.test(s)) { const f = stack.pop(); if (f) frames.push({ ...f, r1: row }); row += 0.4; }
    else if ((m = /^Note\s+(over|left of|right of)\s+([^:]+):\s*(.*)$/i.exec(s))) { const ids = m[2].split(',').map(x => x.trim()); ids.forEach(id => part(id)); rows.push({ note: true, kind: m[1].toLowerCase(), ids, text: mmText(m[3]), r: row }); row += 1; }
    else if ((m = /^([^\s>:-]+)\s*(-->>|->>|-->|->|--x|-x|--\)|-\))\s*[+-]?([^\s:]+)\s*:\s*(.*)$/.exec(s))) {
      part(m[1]); part(m[3]); count++;
      rows.push({ msg: true, from: m[1], to: m[3], arrow: m[2], text: (auto ? count + '. ' : '') + mmText(m[4]), r: row }); row += 1;
    }
  }
  if (!order.length) throw new Error('aucun message trouvé');
  const out = [], xs = new Map(), ROW0 = 130, ROWH = 56;
  const hdr = order.map(id => { const p = parts.get(id); mctx.font = `${MM_FS}px ${FONTS.sans}`; return { id, w: Math.max(120, mctx.measureText(p.label).width + 36) }; });
  let x = 0; hdr.forEach((h, i) => { if (i) x += Math.max(220, (hdr[i - 1].w + h.w) / 2 + 60); xs.set(h.id, x); });
  const yOf = r => ROW0 + r * ROWH, bottom = yOf(row) + 20;
  for (const h of hdr) out.push(...polyline([[xs.get(h.id), 54], [xs.get(h.id), bottom]], { arrowEnd: false, dash: 'dashed', sw: 1.5 }));
  for (const f of frames) {
    const x0 = Math.min(...[...xs.values()]) - 90, x1 = Math.max(...[...xs.values()]) + 90, y0 = yOf(f.r0) - 24, y1 = yOf(f.r1) - 20;
    out.push(mk('rect', { x: x0, y: y0, w: x1 - x0, h: y1 - y0, dash: 'dashed', sw: 1.5, round: false, opacity: 0.7 }));
    const t = mk('text', { text: `${f.kind} ${f.label}`.trim(), fontSize: 14, x: x0 + 8, y: y0 + 4 }); fitText(t); out.push(t);
  }
  for (const mk2 of marks) { const x0 = Math.min(...[...xs.values()]) - 90, x1 = Math.max(...[...xs.values()]) + 90; out.push(...polyline([[x0, yOf(mk2.r) - 12], [x1, yOf(mk2.r) - 12]], { arrowEnd: false, dash: 'dashed', sw: 1.5 })); const t = mk('text', { text: mk2.label, fontSize: 14, x: x0 + 8, y: yOf(mk2.r) - 12 }); fitText(t); out.push(t); }
  for (const h of hdr) {
    const p = parts.get(h.id), o = mk('rect', { x: xs.get(h.id) - h.w / 2, y: 0, w: h.w, h: 54, text: p.label, fontSize: MM_FS, round: true });
    out.push(o);
  }
  for (const r of rows) {
    const y = yOf(r.r);
    if (r.note) {
      const cx = r.ids.map(id => xs.get(id)), mid = (Math.min(...cx) + Math.max(...cx)) / 2, w = Math.max(160, Math.max(...cx) - Math.min(...cx) + 100);
      const sx = r.kind === 'left of' ? Math.min(...cx) - w / 2 - 20 : r.kind === 'right of' ? Math.max(...cx) + w / 2 + 20 : mid;
      const n = mk('sticky', { x: sx - w / 2, y: y - 20, w, h: 54, text: r.text, fill: '#ffec99', sw: 0, fontSize: 15 }); ensureFit(n); out.push(n); continue;
    }
    const a = xs.get(r.from), b = xs.get(r.to), dashed = r.arrow.startsWith('--'), head = /(>>|x|\))$/.test(r.arrow);
    if (a === b) {
      out.push(...polyline([[a, y - 10], [a + 46, y - 10], [a + 46, y + 14], [a, y + 14]], { arrowEnd: true, dash: dashed ? 'dashed' : 'solid', sw: 2.5 }));
      if (r.text) out.push(textAt(r.text, a + 56, y + 2, 15, 'left'));
    } else {
      out.push(...polyline([[a, y], [b, y]], { arrowEnd: head, dash: dashed ? 'dashed' : 'solid', sw: 2.5 }));
      if (r.text) out.push(textAt(r.text, (a + b) / 2, y - 14, 15));
    }
  }
  return out;
}
function mermaidToObjects(src) {
  let lines = src.replace(/\r/g, '').split('\n');
  if (lines[0] && lines[0].trim() === '---') { const e = lines.indexOf('---', 1); if (e > 0) lines = lines.slice(e + 1); }
  lines = lines.map(l => l.replace(/%%.*$/, '').trim()).filter(Boolean);
  if (!lines.length) throw new Error('texte vide');
  const head = lines[0];
  let m;
  if ((m = /^(?:flowchart|graph)(?:-elk)?\b\s*(TD|TB|LR|RL|BT)?/i.exec(head))) {
    const M = newFlow(); M.dir = (m[1] || 'TD').toUpperCase().replace('TB', 'TD');
    parseFlowchart(lines.slice(1), M);
    if (!M.nodes.size) throw new Error('aucun nœud trouvé');
    return mermaidFlow(M);
  }
  if (/^stateDiagram/i.test(head)) {
    const M = newFlow(); M.dir = 'TD'; parseState(lines.slice(1), M);
    if (!M.nodes.size) throw new Error('aucun état trouvé');
    return mermaidFlow(M);
  }
  if (/^sequenceDiagram/i.test(head)) return mermaidSequence(lines.slice(1));
  throw new Error(`type de diagramme non pris en charge (« ${head.split(/\s/)[0]} »). Pris en charge : flowchart/graph, stateDiagram, sequenceDiagram`);
}

const MM_SAMPLE = `flowchart TD
  A[Idée] --> B{Faisable ?}
  B -->|Oui| C[Prototype]
  B -->|Non| D[Abandon]
  C --> E((Livraison))`;
function showMermaid(prefill) {
  openModal(`<h2><span class="grow">Importer du Mermaid</span><button class="pbtn" onclick="closeModal()">Fermer</button></h2>
  <p style="margin:0 0 8px;color:var(--mut)">Collez un diagramme Mermaid : flowchart / graph, stateDiagram ou sequenceDiagram. La mise en page est automatique.</p>
  <textarea id="mmText" spellcheck="false" style="width:100%;height:260px;font:13px Consolas,monospace;background:var(--ui2);color:var(--tx);border:1px solid var(--bd);border-radius:8px;padding:10px;resize:vertical">${esc(prefill || MM_SAMPLE)}</textarea>
  <div style="display:flex;gap:16px;align-items:center;margin-top:10px;flex-wrap:wrap">
    <label><input type="radio" name="mmTarget" value="here" checked> Ajouter à ce tableau</label>
    <label><input type="radio" name="mmTarget" value="new"> Nouveau tableau</label>
    <span style="flex:1"></span><span id="mmErr" style="color:#e03131"></span>
    <button class="pbtn main" id="mmGo">Générer</button></div>`);
  $('#mmGo').onclick = async () => {
    try {
      const objs = mermaidToObjects($('#mmText').value);
      if (!objs.length) throw new Error('rien à dessiner');
      const target = document.querySelector('input[name=mmTarget]:checked').value;
      closeModal();
      if (target === 'new') await loadImported({ objects: objs, name: 'Mermaid' }, 'Mermaid généré'); else { insertImported(objs, {}); toast(`Mermaid généré : ${objs.length} objet(s)`); }
    } catch (e) { $('#mmErr').textContent = 'Erreur : ' + (e.message || e); }
  };
}

/* =====================================================================
   Entrées / sorties fichiers
   ===================================================================== */
function exportDrawioFile() {
  const objs = exportObjs(); if (!objs.length) return toast('Rien à exporter');
  download(new Blob([exportDrawioXml(objs)], { type: 'application/xml' }), fileBase() + '.drawio');
  toast('draw.io exporté' + (objs.some(o => o.type === 'pen') ? ' (traits au crayon exportés comme images)' : ''));
}
function exportExcalidrawFile() {
  const objs = exportObjs(); if (!objs.length) return toast('Rien à exporter');
  download(new Blob([JSON.stringify(exportExcalidrawObj(objs))], { type: 'application/json' }), fileBase() + '.excalidraw');
  toast('Excalidraw exporté');
}
function exportJsonCanvasFile() {
  const objs = exportObjs(); if (!objs.length) return toast('Rien à exporter');
  const { json, rep } = exportJsonCanvasObj(objs);
  if (!json.nodes.length) return toast('JSON Canvas : aucune forme avec texte à exporter');
  download(new Blob([JSON.stringify(json, null, 2)], { type: 'application/json' }), fileBase() + '.canvas');
  toast('JSON Canvas exporté' + (rep.dropped ? ` (${rep.dropped} élément(s) non représentables ignorés)` : ''));
}
async function importAnyFile(file) {
  const ext = (file.name.split('.').pop() || '').toLowerCase(), base = file.name.replace(/\.[^.]+$/, '');
  try {
    const text = await file.text();
    if (ext === 'mmd' || ext === 'mermaid') return showMermaid(text);
    if (ext === 'drawio' || ext === 'xml' || text.trimStart().startsWith('<')) return loadImported(await parseDrawio(text, base), 'draw.io importé');
    const json = JSON.parse(text);
    if (ext === 'excalidraw' || (json && json.type === 'excalidraw')) return loadImported(importExcalidraw(json, base), 'Excalidraw importé');
    if (ext === 'canvas' || (json && Array.isArray(json.nodes) && !json.objects)) return loadImported(importJsonCanvas(json, base), 'JSON Canvas importé');
    if (json && Array.isArray(json.objects)) {
      await saveNow(); loadDoc({ ...json, name: json.name || base }, uid()); markDirty(); await saveNow(); return toast('Tableau importé');
    }
    throw new Error('format non reconnu');
  } catch (e) { console.error(e); toast('Import impossible : ' + (e.message || 'fichier invalide')); }
}
