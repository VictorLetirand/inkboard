'use strict';
/* ===== Icônes ===== */
const ICONS = {
  menu: '<path d="M4 7h16M4 12h16M4 17h16"/>',
  select: '<path d="M5 3l14 8-6 2-3 6z"/>',
  hand: '<path d="M8 13V6.5a1.5 1.5 0 013 0V11M11 11V4.5a1.5 1.5 0 013 0V11M14 11V6.5a1.5 1.5 0 013 0V14c0 4-2.5 7-6.5 7-3 0-4.5-1.5-6.5-4.5L3.5 13a1.5 1.5 0 012.4-1.8L8 14"/>',
  pen: '<path d="M4 20l1-4L16.5 4.5a2 2 0 013 3L8 19z"/><path d="M14 7l3 3"/>',
  highlighter: '<path d="M9 11l4 4M5 19h4l10-10a2 2 0 000-3l-1-1a2 2 0 00-3 0L5 15z"/><path d="M4 21h16"/>',
  eraser: '<path d="M20 20H9L4 15a2 2 0 010-3l8-8a2 2 0 013 0l5 5a2 2 0 010 3l-7 8"/><path d="M8 8l8 8"/>',
  rect: '<rect x="4" y="6" width="16" height="12" rx="2"/>',
  ellipse: '<ellipse cx="12" cy="12" rx="9" ry="7"/>',
  diamond: '<path d="M12 3l9 9-9 9-9-9z"/>',
  triangle: '<path d="M12 4l9 16H3z"/>',
  line: '<path d="M5 19L19 5"/>',
  arrow: '<path d="M5 19L19 5M9 5h10v10"/>',
  text: '<path d="M5 7V4h14v3M12 4v16M9 20h6"/>',
  sticky: '<path d="M4 4h16v10l-6 6H4z"/><path d="M14 20v-6h6"/>',
  image: '<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="M21 16l-5-5L7 20"/>',
  laser: '<circle cx="12" cy="12" r="3"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M18.4 5.6l-2.1 2.1M7.7 16.3l-2.1 2.1"/>',
  undo: '<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 010 12h-3"/>',
  redo: '<path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 000 12h3"/>',
  plus: '<path d="M12 5v14M5 12h14"/>', minus: '<path d="M5 12h14"/>',
  fit: '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>',
  help: '<circle cx="12" cy="12" r="9"/><path d="M9.5 9.5a2.5 2.5 0 115 .5c0 1.5-2.5 2-2.5 3.5M12 17h.01"/>',
  lock: '<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 018 0v3"/>',
  unlock: '<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V8a4 4 0 017.5-2"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
  copy: '<rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 00-2-2H6a2 2 0 00-2 2v8a2 2 0 002 2h2"/>',
  cut: '<circle cx="6" cy="18" r="3"/><circle cx="18" cy="18" r="3"/><path d="M8 16L18 4M16 16L6 4"/>',
  paste: '<rect x="6" y="5" width="12" height="16" rx="2"/><path d="M9 5V4h6v1"/>',
  group: '<rect x="3" y="3" width="8" height="8" rx="1"/><rect x="13" y="13" width="8" height="8" rx="1"/><path d="M11 7h4a2 2 0 012 2v4"/>',
  ungroup: '<rect x="3" y="3" width="8" height="8" rx="1"/><rect x="13" y="13" width="8" height="8" rx="1"/>',
  front: '<rect x="3" y="3" width="11" height="11" rx="1" fill="currentColor" fill-opacity=".25"/><rect x="10" y="10" width="11" height="11" rx="1"/>',
  forward: '<path d="M12 4v12M7 9l5-5 5 5M5 20h14"/>',
  backward: '<path d="M12 20V8M7 15l5 5 5-5M5 4h14"/>',
  back: '<rect x="10" y="10" width="11" height="11" rx="1" fill="currentColor" fill-opacity=".25"/><rect x="3" y="3" width="11" height="11" rx="1"/>',
  al: '<path d="M4 3v18M8 7h12M8 14h8"/>', ac: '<path d="M12 3v18M5 7h14M8 14h8"/>', ar: '<path d="M20 3v18M4 7h12M8 14h8"/>',
  at: '<path d="M3 4h18M7 8v12M14 8v8"/>', am: '<path d="M3 12h18M7 5v14M14 8v8"/>', ab: '<path d="M3 20h18M7 4v12M14 8v8"/>',
  dh: '<path d="M4 4v16M20 4v16M10 8h4v8h-4z"/>', dv: '<path d="M4 4h16M4 20h16M8 10v4h8v-4z"/>',
  folder: '<path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z"/>',
  filenew: '<path d="M6 3h8l5 5v13H6z"/><path d="M14 3v5h5M12 12v6M9 15h6"/>',
  save: '<path d="M5 3h12l3 3v15H5z"/><path d="M8 3v6h8V3M8 21v-7h8v7"/>',
  download: '<path d="M12 4v11M7 11l5 5 5-5M5 20h14"/>',
  grid: '<path d="M4 4h16v16H4zM4 12h16M12 4v16"/>',
  full: '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>',
  edit: '<path d="M4 20l1-4L16.5 4.5a2 2 0 013 3L8 19z"/>',
  all: '<path d="M4 4h16v16H4z" stroke-dasharray="3 3"/>',
  alignL: '<path d="M4 6h16M4 12h10M4 18h14"/>', alignC: '<path d="M4 6h16M7 12h10M5 18h14"/>', alignR: '<path d="M4 6h16M10 12h10M6 18h14"/>',
  trashAll: '<path d="M4 7h16M6 7l1 13h10l1-13M9 7V4h6v3"/>',
};
const ico = n => `<svg viewBox="0 0 24 24">${ICONS[n] || ''}</svg>`;

/* ===== Petits utilitaires UI ===== */
let toastTimer = 0;
function toast(msg) {
  const t = $('#toast'); t.textContent = msg; t.hidden = false;
  clearTimeout(toastTimer); toastTimer = setTimeout(() => (t.hidden = true), 2200);
}
const lsGet = k => { try { return localStorage.getItem(k); } catch (_) { return null; } };
const lsSet = (k, v) => { try { localStorage.setItem(k, v); } catch (_) {} };
function download(blob, name) {
  const a = document.createElement('a'); a.href = URL.createObjectURL(blob); a.download = name;
  document.body.appendChild(a); a.click(); a.remove(); setTimeout(() => URL.revokeObjectURL(a.href), 2000);
}
const fileBase = () => (S.name || 'tableau').replace(/[\\/:*?"<>|]+/g, '_').trim() || 'tableau';

/* ===== Barre d'outils ===== */
function buildToolbar() {
  const tb = $('#toolbar');
  tb.innerHTML = TOOLS.map(t => t.sep ? '<span class="sep"></span>' :
    `<button class="btn" data-tool="${t.id}" title="${t.label} (${t.key.toUpperCase()})">${ico(t.icon)}</button>`).join('') +
    '<span class="sep"></span><button class="btn" id="lockBtn" title="Garder l\'outil actif (Q)"></button>';
  tb.addEventListener('click', e => {
    const b = e.target.closest('button'); if (!b) return;
    if (b.id === 'lockBtn') { S.toolLock = !S.toolLock; updateToolbar(); return; }
    setTool(b.dataset.tool);
  });
  $('#menuBtn').innerHTML = ico('menu');
  $('#undoBtn').innerHTML = ico('undo'); $('#redoBtn').innerHTML = ico('redo');
  $('#zoomOut').innerHTML = ico('minus'); $('#zoomIn').innerHTML = ico('plus'); $('#zoomFit').innerHTML = ico('fit');
  $('#helpBtn').innerHTML = ico('help');
  $('#undoBtn').onclick = undo; $('#redoBtn').onclick = redo;
  $('#zoomOut').onclick = () => zoomBy(1 / 1.25); $('#zoomIn').onclick = () => zoomBy(1.25);
  $('#zoomPct').onclick = resetZoom; $('#zoomFit').onclick = () => fitView();
  $('#helpBtn').onclick = showHelp;
}
function updateToolbar() {
  document.querySelectorAll('#toolbar [data-tool]').forEach(b => b.classList.toggle('active', b.dataset.tool === S.tool));
  const l = $('#lockBtn'); l.innerHTML = ico(S.toolLock ? 'lock' : 'unlock'); l.classList.toggle('active', S.toolLock);
}
function updateHistoryUI() {
  $('#undoBtn').disabled = H.idx <= 0; $('#redoBtn').disabled = H.idx >= H.stack.length - 1;
}
function updateZoomUI() { $('#zoomPct').textContent = Math.round(S.cam.z * 100) + ' %'; }
function viewChanged() { updateZoomUI(); markDirty(); }
function afterChange() { updateHistoryUI(); updatePanel(); markDirty(); }

/* ===== Panneau de propriétés ===== */
const TOOL_TYPES = { pen: ['pen'], highlighter: ['pen'], rect: ['rect'], ellipse: ['ellipse'], diamond: ['diamond'], triangle: ['triangle'], line: ['line'], arrow: ['line'], text: ['text'], sticky: ['sticky'] };
const SW_OPTS = [1.5, 3, 6, 10], SW_HL = [10, 18, 28, 40];
function curVal(k, objs, types) {
  if (objs.length) {
    const vals = objs.filter(o => PROP_TYPES[k].includes(o.type)).map(o => o[k]);
    return vals.length && vals.every(v => v === vals[0]) ? vals[0] : null;
  }
  if (k === 'fill' && S.tool === 'sticky') return S.style.noteFill;
  return S.style[k];
}
function swatches(k, list, cur, label) {
  const items = list.map(v => `<button class="c ${v === 'transparent' ? 'none' : v === 'ink' ? 'ink' : ''} ${cur === v ? 'on' : ''}" style="${v === 'transparent' || v === 'ink' ? '' : 'background:' + v}" data-k="${k}" data-v="${v}" title="${v === 'ink' ? 'Encre' : v === 'transparent' ? 'Aucun' : v}"></button>`).join('');
  const hex = /^#[0-9a-f]{6}$/i.test(cur || '') ? cur : '#4f6df5';
  return `<div class="row"><label>${label}</label><div class="sw">${items}<span class="c custom" title="Couleur personnalisée"><input type="color" data-live="${k}" value="${hex}"></span></div></div>`;
}
function seg(k, opts, cur, label) {
  return `<div class="row"><label>${label}</label><div class="seg">${opts.map(([v, html, title]) =>
    `<button class="${String(cur) === String(v) ? 'on' : ''}" data-k="${k}" data-v="${v}" title="${title || ''}">${html}</button>`).join('')}</div></div>`;
}
const lineIco = (w, dash) => `<svg viewBox="0 0 24 24"><path d="M3 12h18" style="stroke-width:${w};${dash ? 'stroke-dasharray:' + dash : ''}"/></svg>`;
function updatePanel() {
  const p = $('#panel');
  const objs = selObjs();
  let types = objs.length ? [...new Set(objs.map(o => o.type))] : TOOL_TYPES[S.tool];
  if (!types) { p.hidden = true; return; }
  const has = k => types.some(t => PROP_TYPES[k].includes(t));
  const hl = objs.length ? objs.every(o => o.hl) : S.tool === 'highlighter';
  const onlyNotes = types.every(t => t === 'sticky');
  const onlyText = types.every(t => t === 'text');
  let h = '';
  if (has('stroke')) h += swatches('stroke', COLORS, curVal('stroke', objs), onlyText ? 'Couleur du texte' : types.every(t => t === 'pen') ? 'Couleur' : 'Contour');
  if (has('fill')) h += swatches('fill', onlyNotes ? NOTES : FILLS, curVal('fill', objs), onlyNotes ? 'Couleur de la note' : 'Remplissage');
  if (has('sw')) h += seg('sw', (hl ? SW_HL : SW_OPTS).map(v => [v, lineIco(hl ? v / 3 : v * 0.9), 'Épaisseur ' + v]), curVal('sw', objs), 'Épaisseur');
  if (has('dash')) h += seg('dash', [['solid', lineIco(2.2), 'Continu'], ['dashed', lineIco(2.2, '5 3'), 'Tirets'], ['dotted', lineIco(2.6, '0.1 4'), 'Pointillés']], curVal('dash', objs), 'Style de trait');
  if (has('round')) h += seg('round', [['0', ico('rect').replace('rx="2"', 'rx="0"'), 'Angles vifs'], ['1', ico('rect'), 'Arrondis']], curVal('round', objs) ? '1' : curVal('round', objs) === false ? '0' : null, 'Coins');
  if (S.tool === 'arrow' || objs.some(o => o.type === 'line')) {
    const o = objs.find(o => o.type === 'line');
    const cur = o ? (o.arrowStart && o.arrowEnd ? 'both' : o.arrowEnd ? 'end' : o.arrowStart ? 'start' : 'none') : S.style.heads;
    h += seg('heads', [['none', '—', 'Sans pointe'], ['end', '→', 'Pointe à la fin'], ['both', '↔', 'Deux pointes'], ['start', '←', 'Pointe au début']], cur, 'Extrémités');
  }
  if (has('font')) {
    h += seg('font', [['sans', `<span style="font-family:${FONTS.sans}">Aa</span>`, 'Sans empattement'], ['hand', `<span style="font-family:${FONTS.hand}">Aa</span>`, 'Manuscrit'], ['serif', `<span style="font-family:${FONTS.serif}">Aa</span>`, 'Serif'], ['mono', `<span style="font-family:${FONTS.mono}">Aa</span>`, 'Monospace']], curVal('font', objs), 'Police');
    h += seg('fontSize', [[18, 'S'], [24, 'M'], [36, 'L'], [52, 'XL']], curVal('fontSize', objs), 'Taille du texte');
    h += seg('align', [['left', ico('alignL')], ['center', ico('alignC')], ['right', ico('alignR')]], curVal('align', objs), 'Alignement');
  }
  if (has('opacity')) {
    const o = curVal('opacity', objs) ?? 1;
    h += `<div class="row"><label>Opacité <span id="opv">${Math.round(o * 100)}</span> %</label><input type="range" min="5" max="100" value="${Math.round(o * 100)}" data-live="opacity"></div>`;
  }
  if (objs.length) {
    const act = (a, i, t) => `<button class="btn" data-act="${a}" title="${t}">${ico(i)}</button>`;
    const allLocked = objs.every(o => o.locked), grouped = objs.some(o => o.gid);
    h += `<div class="row"><label>Calques</label><div class="seg">${act('front', 'front', 'Premier plan (Ctrl+Maj+])')}${act('forward', 'forward', 'Avancer (Ctrl+])')}${act('backward', 'backward', 'Reculer (Ctrl+[)')}${act('back', 'back', 'Arrière-plan (Ctrl+Maj+[)')}</div></div>`;
    if (objs.length > 1) h += `<div class="row"><label>Alignement</label><div class="seg">${act('al', 'al', 'Aligner à gauche')}${act('ac', 'ac', 'Centrer horizontalement')}${act('ar', 'ar', 'Aligner à droite')}${act('at', 'at', 'Aligner en haut')}${act('am', 'am', 'Centrer verticalement')}${act('ab', 'ab', 'Aligner en bas')}${objs.length > 2 ? act('dh', 'dh', 'Répartir horizontalement') + act('dv', 'dv', 'Répartir verticalement') : ''}</div></div>`;
    h += `<div class="row"><label>Actions</label><div class="seg">${act('dup', 'copy', 'Dupliquer (Ctrl+D)')}${objs.length > 1 || grouped ? act(grouped ? 'ungroup' : 'group', grouped ? 'ungroup' : 'group', grouped ? 'Dégrouper (Ctrl+Maj+G)' : 'Grouper (Ctrl+G)') : ''}${act('lock', allLocked ? 'unlock' : 'lock', allLocked ? 'Déverrouiller' : 'Verrouiller (Ctrl+Maj+L)')}${act('del', 'trash', 'Supprimer (Suppr)')}</div></div>`;
  }
  p.innerHTML = h; p.hidden = !h;
}
const ACTS = {
  front: () => reorder('front'), forward: () => reorder('forward'), backward: () => reorder('backward'), back: () => reorder('back'),
  al: () => alignSel('l'), ac: () => alignSel('c'), ar: () => alignSel('r'), at: () => alignSel('t'), am: () => alignSel('m'), ab: () => alignSel('b'),
  dh: () => distributeSel('h'), dv: () => distributeSel('v'), dup: duplicateSel, group: groupSel, ungroup: ungroupSel, lock: toggleLock, del: deleteSel,
};
(function initPanel() {
  const p = $('#panel');
  p.addEventListener('pointerdown', e => { if (e.target.tagName !== 'INPUT') e.preventDefault(); });
  p.addEventListener('click', e => {
    const b = e.target.closest('button'); if (!b) return;
    if (b.dataset.act) return ACTS[b.dataset.act]();
    if (b.dataset.k) {
      const k = b.dataset.k; let v = b.dataset.v;
      if (k === 'sw' || k === 'fontSize') v = Number(v); else if (k === 'round') v = v === '1';
      applyProp(k, v);
    }
  });
  p.addEventListener('input', e => {
    const k = e.target.dataset.live; if (!k) return;
    const v = k === 'opacity' ? e.target.value / 100 : e.target.value;
    if (k === 'opacity') $('#opv').textContent = e.target.value;
    S.style[k === 'fill' && selObjs().length && selObjs().every(o => o.type === 'sticky') ? 'noteFill' : k] = v;
    for (const o of selObjs().filter(unlocked)) if (PROP_TYPES[k].includes(o.type)) o[k] = v;
    requestRender();
  });
  p.addEventListener('change', e => { const k = e.target.dataset.live; if (k) { const v = k === 'opacity' ? e.target.value / 100 : e.target.value; applyProp(k, v); } });
})();

/* ===== Menus ===== */
function renderMenu(el, items) {
  el.innerHTML = items.map((it, i) => {
    if (it.sep) return '<hr>';
    if (it.header) return `<div class="lbl">${it.header}</div>`;
    if (it.chips) return `<div class="chips">${it.chips.map(c => `<button class="chip ${c.on ? 'on' : ''}" style="background:${c.c}" title="${c.name}" data-i="${i}" data-c="${c.id}"></button>`).join('')}</div>`;
    return `<button class="it ${it.on ? 'on' : ''}" data-i="${i}" ${it.disabled ? 'disabled' : ''}>${it.icon ? ico(it.icon) : '<span style="width:17px"></span>'}<span>${it.label}</span>${it.key ? `<span class="k">${it.key}</span>` : ''}</button>`;
  }).join('');
  el.onclick = e => {
    const b = e.target.closest('[data-i]'); if (!b) return;
    const it = items[+b.dataset.i];
    if (it.chips) { it.pick(b.dataset.c); renderMenu(el, it.rebuild ? it.rebuild() : items); return; }
    closeMenus(); it.fn && it.fn();
  };
}
function closeMenus() { $('#menu').hidden = true; $('#ctx').hidden = true; }
function mainMenuItems() {
  const sel = S.sel.size > 0;
  const build = () => [
    { icon: 'filenew', label: 'Nouveau tableau', fn: () => newBoard() },
    { icon: 'folder', label: 'Mes tableaux…', fn: showBoards },
    { icon: 'folder', label: 'Ouvrir / importer un fichier…', key: 'Ctrl+O', fn: () => $('#fileOpen').click() },
    { icon: 'filenew', label: 'Importer du Mermaid…', fn: () => showMermaid() },
    { icon: 'save', label: 'Enregistrer dans un fichier', key: 'Ctrl+S', fn: saveFile },
    { sep: 1 },
    { icon: 'download', label: sel ? 'Exporter la sélection en PNG' : 'Exporter en PNG', key: 'Ctrl+E', fn: () => exportPNG(false) },
    { icon: 'download', label: 'Exporter en PNG (fond transparent)', fn: () => exportPNG(true) },
    { icon: 'download', label: 'Exporter en SVG', fn: exportSVGFile },
    { icon: 'copy', label: 'Copier l\'image dans le presse-papiers', fn: copyPNG },
    { header: 'Échange avec d\'autres outils' },
    { icon: 'download', label: 'Exporter en draw.io (.drawio)', fn: exportDrawioFile },
    { icon: 'download', label: 'Exporter en Excalidraw', fn: exportExcalidrawFile },
    { icon: 'download', label: 'Exporter en JSON Canvas (.canvas)', fn: exportJsonCanvasFile },
    { sep: 1 },
    { header: 'Arrière-plan' },
    { chips: BACKGROUNDS.map(b => ({ ...b, on: b.id === S.bg })), pick: id => { S.bg = id; applyTheme(); markDirty(); requestRender(); }, rebuild: build },
    ...PATTERNS.map(([id, n]) => ({ icon: 'grid', label: n, on: S.pattern === id, fn: () => { S.pattern = id; markDirty(); requestRender(); } })),
    { icon: 'grid', label: 'Magnétisme de la grille', on: S.snap, fn: () => { S.snap = !S.snap; toast(S.snap ? 'Magnétisme activé' : 'Magnétisme désactivé'); } },
    { sep: 1 },
    { icon: 'full', label: 'Plein écran', fn: () => (document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen().catch(() => {})) },
    { icon: 'help', label: 'Raccourcis clavier', key: '?', fn: showHelp },
    { icon: 'trashAll', label: 'Effacer tout le tableau', fn: clearBoard },
  ];
  return build();
}
$('#menuBtn').addEventListener('click', e => {
  e.stopPropagation();
  const m = $('#menu');
  if (!m.hidden) return (m.hidden = true);
  $('#ctx').hidden = true; renderMenu(m, mainMenuItems()); m.hidden = false;
});
document.addEventListener('pointerdown', e => { if (!e.target.closest('.menu, #menuBtn')) closeMenus(); });
function showContextMenu(x, y) {
  const n = S.sel.size, objs = selObjs(), grouped = objs.some(o => o.gid), allLocked = n && objs.every(o => o.locked);
  const items = n ? [
    { icon: 'cut', label: 'Couper', key: 'Ctrl+X', fn: () => { copySel(); deleteSel(); } },
    { icon: 'copy', label: 'Copier', key: 'Ctrl+C', fn: copySel },
    { icon: 'paste', label: 'Coller', key: 'Ctrl+V', disabled: !clip, fn: pasteClip },
    { icon: 'copy', label: 'Dupliquer', key: 'Ctrl+D', fn: duplicateSel },
    { sep: 1 },
    { icon: 'front', label: 'Premier plan', fn: () => reorder('front') },
    { icon: 'back', label: 'Arrière-plan', fn: () => reorder('back') },
    { sep: 1 },
    ...(n > 1 ? [{ icon: 'group', label: 'Grouper', key: 'Ctrl+G', fn: groupSel }] : []),
    ...(grouped ? [{ icon: 'ungroup', label: 'Dégrouper', key: 'Ctrl+Maj+G', fn: ungroupSel }] : []),
    { icon: allLocked ? 'unlock' : 'lock', label: allLocked ? 'Déverrouiller' : 'Verrouiller', fn: toggleLock },
    { sep: 1 },
    { icon: 'trash', label: 'Supprimer', key: 'Suppr', fn: deleteSel },
  ] : [
    { icon: 'paste', label: 'Coller', key: 'Ctrl+V', disabled: !clip, fn: pasteClip },
    { icon: 'all', label: 'Tout sélectionner', key: 'Ctrl+A', fn: selectAll },
    { icon: 'fit', label: 'Tout afficher', key: 'Ctrl+1', fn: () => fitView() },
  ];
  const el = $('#ctx'); $('#menu').hidden = true;
  renderMenu(el, items); el.hidden = false;
  el.style.left = Math.min(x, VW - el.offsetWidth - 8) + 'px'; el.style.top = Math.min(y, VH - el.offsetHeight - 8) + 'px';
}

/* ===== Modales ===== */
function openModal(html) { $('#modalBox').innerHTML = html; $('#modal').hidden = false; }
function closeModal() { $('#modal').hidden = true; $('#modalBox').innerHTML = ''; }
$('#modal').addEventListener('pointerdown', e => { if (e.target.id === 'modal') closeModal(); });
function showHelp() {
  const rows = [
    ['Outils'], ['Sélection', 'V'], ['Main', 'H ou Espace + glisser'], ['Crayon', 'P'], ['Surligneur', 'M'], ['Gomme', 'E'], ['Rectangle', 'R'], ['Ellipse', 'O'], ['Losange', 'D'], ['Triangle', 'Y'],
    ['Ligne', 'L'], ['Flèche', 'A'], ['Texte', 'T (ou double-clic)'], ['Note adhésive', 'N'], ['Image', 'I (ou glisser-déposer / coller)'], ['Pointeur laser', 'F'], ['Garder l\'outil actif', 'Q'],
    ['Édition'], ['Annuler / Rétablir', 'Ctrl+Z / Ctrl+Y'], ['Copier / Couper / Coller', 'Ctrl+C / X / V'], ['Dupliquer', 'Ctrl+D ou Alt+glisser'], ['Tout sélectionner', 'Ctrl+A'],
    ['Supprimer', 'Suppr'], ['Grouper / Dégrouper', 'Ctrl+G / Ctrl+Maj+G'], ['Verrouiller', 'Ctrl+Maj+L'], ['Déplacer finement', 'Flèches (Maj = ×10)'],
    ['Modifier le texte', 'Entrée ou double-clic'], ['Avancer / Reculer', 'Ctrl+] / Ctrl+['], ['Premier plan / Arrière', 'Ctrl+Maj+] / ['],
    ['Dessin'], ['Ligne droite 15°', 'Maj pendant le tracé'], ['Forme carrée / cercle', 'Maj pendant le tracé'], ['Dessiner depuis le centre', 'Alt pendant le tracé'], ['Rotation par pas de 15°', 'Maj'],
    ['Vue'], ['Zoom', 'Molette / Ctrl+molette / pincement'], ['Zoom avant / arrière', 'Ctrl++ / Ctrl+-'], ['Zoom 100 %', 'Ctrl+0'], ['Tout afficher', 'Ctrl+1'],
    ['Fichier'], ['Enregistrer dans un fichier', 'Ctrl+S'], ['Ouvrir un fichier', 'Ctrl+O'], ['Exporter en PNG', 'Ctrl+E'],
  ];
  openModal(`<h2><span class="grow">Raccourcis clavier</span><button class="pbtn" onclick="closeModal()">Fermer</button></h2><div class="keys">${rows.map(r => r.length === 1 ? `<h3>${r[0]}</h3>` : `<div><span>${r[0]}</span><kbd>${r[1]}</kbd></div>`).join('')}</div>`);
}

/* ===== Stockage (IndexedDB) ===== */
const DB = {
  db: null,
  open() {
    return new Promise((res, rej) => {
      const r = indexedDB.open('tableau-blanc', 1);
      r.onupgradeneeded = () => { r.result.createObjectStore('boards', { keyPath: 'id' }); r.result.createObjectStore('meta', { keyPath: 'id' }); };
      r.onsuccess = () => { this.db = r.result; res(); };
      r.onerror = () => rej(r.error);
    });
  },
  op(store, mode, fn) {
    return new Promise((res, rej) => {
      const t = this.db.transaction(store, mode), rq = fn(t.objectStore(store));
      t.oncomplete = () => res(rq && rq.result); t.onerror = () => rej(t.error); t.onabort = () => rej(t.error);
    });
  },
  get(s, id) { return this.op(s, 'readonly', st => st.get(id)); },
  put(s, v) { return this.op(s, 'readwrite', st => st.put(v)); },
  del(s, id) { return this.op(s, 'readwrite', st => st.delete(id)); },
  all(s) { return this.op(s, 'readonly', st => st.getAll()); },
};
let saveTimer = 0, dirty = false;
function setSaveState(t) { $('#saveState').textContent = t; }
function markDirty() {
  dirty = true; setSaveState(DB.db ? 'Modifications…' : 'Non enregistré');
  clearTimeout(saveTimer); saveTimer = setTimeout(saveNow, 800);
}
function makeThumb() {
  if (!S.objects.length) return '';
  const c = exportCanvas(S.objects, { scale: 0.4, pad: 20, maxDim: 900 }); if (!c) return '';
  const t = document.createElement('canvas'); t.width = 240; t.height = 150;
  const x = t.getContext('2d'); x.fillStyle = bgInfo().c; x.fillRect(0, 0, 240, 150);
  const k = Math.min(240 / c.width, 150 / c.height);
  x.drawImage(c, (240 - c.width * k) / 2, (150 - c.height * k) / 2, c.width * k, c.height * k);
  return t.toDataURL('image/jpeg', 0.6);
}
async function saveNow() {
  clearTimeout(saveTimer);
  if (!DB.db || !S.id) return;
  dirty = false;
  try {
    const id = S.id;
    await DB.put('boards', { id, doc: serialize() });
    await DB.put('meta', { id, name: S.name, updated: Date.now(), thumb: makeThumb() });
    if (!dirty) setSaveState('Enregistré');
  } catch (e) { setSaveState('Erreur de sauvegarde'); console.error(e); }
}
window.addEventListener('beforeunload', () => { if (dirty) saveNow(); });
document.addEventListener('visibilitychange', () => { if (document.hidden && dirty) saveNow(); });

function applyTheme() {
  const dark = bgInfo().dark;
  document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  const m = document.querySelector('meta[name=theme-color]'); if (m) m.content = bgInfo().c;
}
function loadDoc(doc, id) {
  S.id = id; S.name = doc.name || 'Sans titre'; S.bg = doc.bg || 'white'; S.pattern = doc.pattern || 'dots';
  S.objects = Array.isArray(doc.objects) ? doc.objects : []; S.assets = doc.assets || {};
  S.cam = doc.cam || { x: VW / 2, y: VH / 2, z: 1 };
  S.sel = new Set(); S.draft = null; S.editing = null; $('#editor').hidden = true;
  IMG.clear(); resetHistory(); applyTheme();
  $('#boardName').value = S.name; updateZoomUI(); updatePanel(); updateToolbar(); setSaveState(DB.db ? 'Enregistré' : '');
  lsSet('tb.current', id); requestRender();
}
async function newBoard(name) {
  await saveNow();
  const id = uid();
  loadDoc({ name: name || 'Sans titre', bg: S.bg, pattern: S.pattern, objects: [], assets: {}, cam: { x: VW / 2, y: VH / 2, z: 1 } }, id);
  await saveNow(); setTool('select');
}
async function openBoard(id) {
  if (!DB.db) return false;
  const rec = await DB.get('boards', id); if (!rec) return false;
  await saveNow(); loadDoc(rec.doc, id); return true;
}
const importBoardFile = file => importAnyFile(file); /* formats.js */
$('#fileOpen').addEventListener('change', e => { const f = e.target.files[0]; e.target.value = ''; if (f) importBoardFile(f); });
function saveFile() {
  download(new Blob([JSON.stringify(serialize())], { type: 'application/json' }), fileBase() + '.tableau.json'); toast('Fichier enregistré');
}
function exportObjs() { return S.sel.size ? selObjs() : S.objects; }
function exportPNG(transparent) {
  const objs = exportObjs();
  const c = exportCanvas(objs, { scale: 2, transparent: !!transparent });
  if (!c) return toast('Rien à exporter');
  c.toBlob(b => download(b, fileBase() + '.png'), 'image/png');
}
function exportSVGFile() {
  const s = exportSVG(exportObjs());
  if (!s) return toast('Rien à exporter');
  download(new Blob([s], { type: 'image/svg+xml' }), fileBase() + '.svg');
}
function copyPNG() {
  const c = exportCanvas(exportObjs(), { scale: 2 }); if (!c) return toast('Rien à copier');
  c.toBlob(async b => {
    try { await navigator.clipboard.write([new ClipboardItem({ 'image/png': b })]); toast('Image copiée'); }
    catch (_) { toast('Copie impossible : utilisez l\'export PNG'); }
  }, 'image/png');
}
function clearBoard() {
  if (!S.objects.length) return;
  if (!confirm('Effacer tout le contenu du tableau ? (annulable avec Ctrl+Z)')) return;
  S.objects = []; S.sel = new Set(); commit(); requestRender();
}

/* ===== Gestionnaire de tableaux ===== */
async function showBoards() {
  await saveNow();
  let metas = [];
  try { metas = DB.db ? await DB.all('meta') : []; } catch (_) {}
  metas.sort((a, b) => b.updated - a.updated);
  const fmt = t => new Date(t).toLocaleString('fr-FR', { dateStyle: 'medium', timeStyle: 'short' });
  openModal(`<h2><span class="grow">Mes tableaux</span><button class="pbtn main" id="bNew">Nouveau</button><button class="pbtn" id="bImp">Importer un fichier</button><button class="pbtn" onclick="closeModal()">Fermer</button></h2>
  <div class="boards">${metas.map(m => `<div class="bcard ${m.id === S.id ? 'cur' : ''}" data-id="${m.id}">
    <div class="th" style="${m.thumb ? `background-image:url(${m.thumb})` : ''}"></div><div class="nm">${esc(m.name)}</div><div class="dt">${fmt(m.updated)}</div>
    <div class="acts"><button class="btn" data-a="ren" title="Renommer">${ico('edit')}</button><button class="btn" data-a="dup" title="Dupliquer">${ico('copy')}</button><button class="btn" data-a="del" title="Supprimer">${ico('trash')}</button></div></div>`).join('') || '<p>Aucun tableau enregistré.</p>'}</div>`);
  $('#bNew').onclick = async () => { closeModal(); await newBoard(); };
  $('#bImp').onclick = () => { closeModal(); $('#fileOpen').click(); };
  $('#modalBox .boards').onclick = async e => {
    const card = e.target.closest('.bcard'); if (!card) return;
    const id = card.dataset.id, act = e.target.closest('[data-a]')?.dataset.a;
    if (!act) { closeModal(); if (id !== S.id) await openBoard(id); return; }
    const meta = metas.find(m => m.id === id);
    if (act === 'ren') {
      const n = prompt('Nouveau nom :', meta.name); if (!n) return;
      meta.name = n.slice(0, 80); await DB.put('meta', meta);
      if (id === S.id) { S.name = meta.name; $('#boardName').value = S.name; markDirty(); }
      else { const rec = await DB.get('boards', id); rec.doc.name = meta.name; await DB.put('boards', rec); }
    } else if (act === 'dup') {
      const rec = await DB.get('boards', id), nid = uid();
      await DB.put('boards', { id: nid, doc: { ...rec.doc, name: meta.name + ' (copie)' } });
      await DB.put('meta', { ...meta, id: nid, name: meta.name + ' (copie)', updated: Date.now() });
    } else if (act === 'del') {
      if (!confirm(`Supprimer définitivement « ${meta.name} » ?`)) return;
      await DB.del('boards', id); await DB.del('meta', id);
      if (id === S.id) {
        const rest = (await DB.all('meta')).sort((a, b) => b.updated - a.updated);
        if (rest[0]) await openBoard(rest[0].id); else { S.id = null; await newBoard(); }
      }
    }
    showBoards();
  };
}
$('#boardName').addEventListener('input', e => { S.name = e.target.value; markDirty(); });
$('#boardName').addEventListener('keydown', e => { e.stopPropagation(); if (e.key === 'Enter') e.target.blur(); });

/* ===== Démarrage ===== */
(async function boot() {
  buildToolbar(); updateToolbar(); resizeCanvas(); updateHistoryUI();
  try { await DB.open(); } catch (e) { toast('Sauvegarde automatique indisponible : pensez à exporter vos tableaux'); }
  let ok = false;
  const cur = lsGet('tb.current');
  if (cur && DB.db) { try { const rec = await DB.get('boards', cur); if (rec) { loadDoc(rec.doc, cur); ok = true; } } catch (_) {} }
  if (!ok) loadDoc({ name: 'Sans titre', objects: [], assets: {}, cam: { x: VW / 2, y: VH / 2, z: 1 } }, uid());
  if (!ok && DB.db) saveNow();
  if ('serviceWorker' in navigator && location.protocol.startsWith('http')) navigator.serviceWorker.register('sw.js').catch(() => {});
})();
