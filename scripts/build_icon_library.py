#!/usr/bin/env python3
"""Builds the icon library embedded in Inkboard: crates/inkboard-app/assets/icons.bin and crates/inkboard-app/assets/ICONS-LICENSES.txt.

Usage: python scripts/build_icon_library.py SRC_DIR

SRC_DIR contains shallow clones of the upstream projects (see docs/ICONS.md for the exact commands):
  drawio/         jgraph/drawio    (Apache-2.0)  src/main/webapp/js/stencils.min.js and src/main/webapp/img/lib
  simple-icons/   simple-icons     (CC0-1.0)     icons/*.svg and data/simple-icons.json
  tabler/         tabler-icons     (MIT)         icons/outline/*.svg
  lucide/         lucide           (ISC)         icons/*.svg
  mdi/            MaterialDesign-SVG (Apache-2.0) svg/*.svg

Pack layout (little endian): b"INKLIB01", u32 length of the deflated JSON index, the index, then one raw-deflate chunk per set.
A chunk is a sequence of records `u32 length, SVG bytes`, in the order of the index's `names`.
"""
import base64
import json
import os
import re
import struct
import sys
import zlib
from pathlib import Path
from xml.sax.saxutils import quoteattr

sys.stdout.reconfigure(encoding='utf-8')

ROOT = Path(__file__).resolve().parent.parent
OUT_BIN = ROOT / 'crates' / 'inkboard-app' / 'assets' / 'icons.bin'
OUT_LIC = ROOT / 'crates' / 'inkboard-app' / 'assets' / 'ICONS-LICENSES.txt'

# ---------------------------------------------------------------------------------------------------------- draw.io stencil decoder

DRAW = {'move', 'line', 'quad', 'curve', 'arc', 'rect', 'roundrect', 'ellipse', 'text', 'image', 'include-shape'}
XS = {'x', 'x1', 'x2', 'x3'}
YS = {'y', 'y1', 'y2', 'y3'}


class ZeroDict(dict):
    """Attributes of a stencil element: a coordinate that is not given is 0."""

    def __missing__(self, key):
        return '0'


class Node:
    def __init__(self, name, parent):
        self.name = name
        self.parent = parent
        self.attrs = {}
        self.children = []


def _fmt(n):
    s = ('%.3f' % (n / 1000)).rstrip('0').rstrip('.')
    return s if s not in ('', '-0') else '0'


def decode_stencil_set(b64):
    """Port of `decode()` of draw.io's stencils.min.js (a compact binary encoding of the stencil XML)."""
    d = zlib.decompress(base64.b64decode(b64), -15)
    sizes = struct.unpack('>5I', d[:20])
    streams = []
    o = 20
    for n in sizes:
        streams.append(d[o:o + n])
        o += n
    ops = streams[0]
    strs = streams[4].decode('utf-8').split('\0')
    pos = [0, 0, 0, 0]
    si = 0
    names, attr_names = [], []
    root = Node(None, None)
    parent = root
    px = py = 0
    pn = {}

    def read(k):
        data, n, m = streams[k], 0, 1
        while True:
            c = data[pos[k]]
            pos[k] += 1
            n += (c & 127) * m
            m *= 128
            if not c & 128:
                break
        return -(n + 1) // 2 if n % 2 == 1 else n // 2

    while pos[0] < len(ops):
        op = ops[pos[0]]
        pos[0] += 1
        if op == 255:
            parent = parent.parent
            continue
        if (op >> 1) == len(names):
            names.append(strs[si])
            si += 1
        name = names[op >> 1]
        node = Node(name, parent)
        count = ops[pos[0]]
        pos[0] += 1
        coords = name in DRAW
        parent.children.append(node)
        if name == 'shape':
            px = py = 0
            pn = {}
        for _ in range(count):
            aid = ops[pos[0]]
            pos[0] += 1
            if aid == len(attr_names):
                attr_names.append(strs[si])
                si += 1
            attr = attr_names[aid]
            kind = ops[pos[0]]
            pos[0] += 1
            if kind == 0:
                node.attrs[attr] = strs[si]
                si += 1
            elif coords and attr in XS:
                px += read(1)
                node.attrs[attr] = _fmt(px)
            elif coords and attr in YS:
                py += read(2)
                node.attrs[attr] = _fmt(py)
            else:
                key = name + ' ' + attr
                nxt = pn.get(key, 0) + read(3)
                pn[key] = nxt
                node.attrs[attr] = _fmt(nxt)
        if not op & 1:
            parent = node
    return root.children[0] if root.children else None


def load_stencil_sets(drawio_dir):
    text = (Path(drawio_dir) / 'src/main/webapp/js/stencils.min.js').read_text(encoding='utf-8')
    sets = {}
    for m in re.finditer(r"f\['([^']+)'\]\s*=\s*'([^']*)'", text):
        sets[m.group(1)[:-4].replace('/', '_')] = decode_stencil_set(m.group(2))
    return sets


# ------------------------------------------------------------------------------------------------------------ stencil -> SVG

WHITE = '#ffffff'


def num(x):
    s = ('%.2f' % float(x)).rstrip('0').rstrip('.')
    return s if s not in ('', '-0') else '0'


def luma(color):
    m = re.fullmatch(r'#?([0-9a-fA-F]{6})', color or '')
    if not m:
        return 0.0
    r, g, b = (int(m.group(1)[i:i + 2], 16) for i in (0, 2, 4))
    return (0.299 * r + 0.587 * g + 0.114 * b) / 255


def stencil_to_svg(shape, style, brand):
    """`style`: 'outline' (diagram shape: faint fill, full outline) or 'solid' (icon: solid glyph). `brand`: color baked in, or None for a white SVG
    that the application tints with the object's stroke color."""
    w, h = float(shape.attrs.get('w', 100)), float(shape.attrs.get('h', 100))
    if w <= 0 or h <= 0:
        return None
    base_sw = min(4.0, max(1.0, max(w, h) / 50))
    pad = base_sw / 2 + 0.5
    state = dict(fill=None, stroke=None, sw=base_sw, dash=None, cap=None, join='round', fa=1.0, sa=1.0, a=1.0)
    stack = []
    out = []
    path = []
    pending = None  # ('rect'|'ellipse'|'roundrect', attrs) waiting for fill / stroke

    def color_for(value, kind):
        """Resolved color and opacity factor for an explicit color of the stencil, or the default one."""
        if brand:
            return (value, 1.0) if value and value != 'none' else (brand, 1.0)
        if value is None:
            if kind == 'fill':
                return (WHITE, 0.14 if style == 'outline' else 1.0)
            return (WHITE, 1.0)
        if value == 'none':
            return ('none', 1.0)
        dark = luma(value) < 0.5
        if kind == 'fill':
            return (WHITE, 1.0 if dark else 0.18)
        return (WHITE, 1.0 if dark else 0.5)

    def emit(fill, stroke):
        nonlocal path, pending
        if pending is not None:
            tag, geo = pending
            body = f'<{tag} {geo}'
        elif path:
            body = f'<path d="{" ".join(path)}"'
        else:
            return
        fc, fo = color_for(state['fill'], 'fill') if fill else ('none', 1.0)
        sc, so = color_for(state['stroke'], 'stroke') if stroke else ('none', 1.0)
        attrs = f' fill="{fc}"'
        if fc != 'none':
            op = state['fa'] * state['a'] * fo
            if op < 0.999:
                attrs += f' fill-opacity="{num(op)}"'
        attrs += f' stroke="{sc}"'
        if sc != 'none':
            attrs += f' stroke-width="{num(state["sw"])}"'
            op = state['sa'] * state['a'] * so
            if op < 0.999:
                attrs += f' stroke-opacity="{num(op)}"'
            if state['dash']:
                attrs += f' stroke-dasharray="{state["dash"]}"'
            attrs += f' stroke-linejoin="{state["join"]}"'
            if state['cap']:
                attrs += f' stroke-linecap="{state["cap"]}"'
        out.append(body + attrs + '/>')
        path = []
        pending = None

    def walk(node):
        nonlocal path, pending
        for c in node.children:
            n, a = c.name, ZeroDict(c.attrs)
            if n == 'path':
                path = []
                pending = None
                walk(c)
            elif n == 'move':
                path.append(f'M{num(a["x"])} {num(a["y"])}')
            elif n == 'line':
                path.append(f'L{num(a["x"])} {num(a["y"])}')
            elif n == 'quad':
                path.append(f'Q{num(a["x1"])} {num(a["y1"])} {num(a["x2"])} {num(a["y2"])}')
            elif n == 'curve':
                path.append(f'C{num(a["x1"])} {num(a["y1"])} {num(a["x2"])} {num(a["y2"])} {num(a["x3"])} {num(a["y3"])}')
            elif n == 'arc':
                path.append(f'A{num(a["rx"])} {num(a["ry"])} {num(a.get("x-axis-rotation", 0))} {int(float(a.get("large-arc-flag", 0)))} {int(float(a.get("sweep-flag", 0)))} {num(a["x"])} {num(a["y"])}')
            elif n == 'close':
                path.append('Z')
            elif n == 'rect':
                pending = ('rect', f'x="{num(a["x"])}" y="{num(a["y"])}" width="{num(a["w"])}" height="{num(a["h"])}"')
                path = []
            elif n == 'roundrect':
                aw, ah = float(a['w']), float(a['h'])
                r = min(aw, ah) * float(a.get('arcsize', 10)) / 100
                pending = ('rect', f'x="{num(a["x"])}" y="{num(a["y"])}" width="{num(aw)}" height="{num(ah)}" rx="{num(r)}"')
                path = []
            elif n == 'ellipse':
                x, y, ew, eh = (float(a[k]) for k in ('x', 'y', 'w', 'h'))
                pending = ('ellipse', f'cx="{num(x + ew / 2)}" cy="{num(y + eh / 2)}" rx="{num(ew / 2)}" ry="{num(eh / 2)}"')
                path = []
            elif n == 'fillstroke':
                emit(True, True)
            elif n == 'fill':
                emit(True, False)
            elif n == 'stroke':
                emit(False, True)
            elif n == 'fillcolor':
                state['fill'] = a.get('color')
            elif n == 'strokecolor':
                state['stroke'] = a.get('color')
            elif n == 'fillstrokecolor':
                state['fill'] = state['stroke'] = a.get('color')
            elif n == 'strokewidth':
                state['sw'] = max(0.3, float(a.get('width', base_sw)))
            elif n == 'dashed':
                on = a.get('dashed') == '1'
                state['dash'] = (state['dash'] or '3 3') if on else None
            elif n == 'dashpattern':
                state['dash'] = a.get('pattern')
            elif n == 'linecap':
                state['cap'] = a.get('cap')
            elif n == 'linejoin':
                state['join'] = a.get('join', 'round')
            elif n == 'alpha':
                state['a'] = float(a.get('alpha', 1))
            elif n == 'fillalpha':
                state['fa'] = float(a.get('alpha', 1))
            elif n == 'strokealpha':
                state['sa'] = float(a.get('alpha', 1))
            elif n == 'save':
                stack.append(dict(state))
            elif n == 'restore':
                if stack:
                    state.update(stack.pop())
            elif n in ('background', 'foreground'):
                walk(c)

    for part in shape.children:
        if part.name in ('background', 'foreground'):
            walk(part)
    if not out:
        return None
    vb = f'{num(-pad)} {num(-pad)} {num(w + 2 * pad)} {num(h + 2 * pad)}'
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" width="{num(w + 2 * pad)}" height="{num(h + 2 * pad)}">{"".join(out)}</svg>'


# ----------------------------------------------------------------------------------------------------------------- SVG cleanup


def clean_svg(text, extra_root_attrs=''):
    text = re.sub(r'<\?xml[^>]*\?>', '', text)
    text = re.sub(r'<!DOCTYPE[^>]*>', '', text, flags=re.S)
    text = re.sub(r'<!--.*?-->', '', text, flags=re.S)
    text = re.sub(r'<metadata.*?</metadata>', '', text, flags=re.S)
    text = re.sub(r'<(sodipodi|inkscape):[^>]*/>', '', text)
    text = re.sub(r'\s(sodipodi|inkscape):[a-zA-Z\-]+="[^"]*"', '', text)
    text = re.sub(r'>\s+<', '><', text.strip())
    text = re.sub(r'\s+', ' ', text)
    if extra_root_attrs:
        text = text.replace('<svg', '<svg ' + extra_root_attrs, 1)
    return text


def mono_stroke_icon(text):
    """Tabler / Lucide outline icons: `currentColor` strokes become white (tinted by the application)."""
    text = clean_svg(text)
    text = text.replace('currentColor', WHITE)
    text = re.sub(r'<path stroke="none" d="M0 0h24v24H0z" fill="none"/>', '', text)
    text = re.sub(r'\sclass="[^"]*"', '', text)
    return text


# --------------------------------------------------------------------------------------------------------------- sets definitions

# (stem prefix, group, title, style, brand color): first match wins; style 'outline' = diagram shapes, 'solid' = icons
STENCIL_RULES = [
    ('aws4', 'AWS', 'AWS architecture icons', 'solid', '#ED7100'),
    ('aws3d', 'AWS', 'AWS 3D', 'solid', '#ED7100'),
    ('aws3', 'AWS', 'AWS 2017', 'solid', '#ED7100'),
    ('aws2', 'AWS', 'AWS 2014', 'solid', '#ED7100'),
    ('aws_', 'AWS', 'AWS 2012', 'solid', '#ED7100'),
    ('azure', 'Microsoft Azure', 'Azure', 'solid', '#0078D4'),
    ('mscae', 'Microsoft Azure', 'Microsoft Cloud and Enterprise', 'solid', '#0078D4'),
    ('office_', 'Microsoft Office', 'Microsoft Office', 'solid', None),
    ('gcp', 'Google Cloud', 'Google Cloud', 'solid', '#4285F4'),
    ('cisco_safe', 'Cisco', 'Cisco SAFE', 'solid', '#049FD9'),
    ('cisco', 'Cisco', 'Cisco', 'solid', '#049FD9'),
    ('rack_cisco', 'Cisco', 'Cisco rack equipment', 'solid', None),
    ('ibm', 'IBM', 'IBM', 'solid', '#1F70C1'),
    ('kubernetes', 'Cloud and IT', 'Kubernetes', 'solid', '#326CE5'),
    ('alibaba', 'Cloud and IT', 'Alibaba Cloud', 'solid', '#FF6A00'),
    ('atlassian', 'Cloud and IT', 'Atlassian', 'solid', '#0052CC'),
    ('salesforce', 'Cloud and IT', 'Salesforce', 'solid', '#00A1E0'),
    ('openstack', 'Cloud and IT', 'OpenStack', 'solid', '#ED1944'),
    ('veeam', 'Cloud and IT', 'Veeam', 'solid', '#00B336'),
    ('citrix', 'Cloud and IT', 'Citrix', 'solid', None),
    ('vvd', 'Cloud and IT', 'VMware', 'solid', None),
    ('networks', 'Cloud and IT', 'Networking', 'solid', None),
    ('rack_', 'Cloud and IT', 'Rack equipment', 'solid', None),
    ('weblogos', 'Cloud and IT', 'Web logos', 'solid', None),
    ('webicons', 'Cloud and IT', 'Web icons', 'solid', None),
    ('basic', 'Diagram shapes', 'Basic shapes', 'outline', None),
    ('flowchart', 'Diagram shapes', 'Flowchart', 'outline', None),
    ('bpmn', 'Diagram shapes', 'BPMN', 'outline', None),
    ('arrows', 'Diagram shapes', 'Arrows', 'outline', None),
    ('eip', 'Diagram shapes', 'Enterprise integration patterns', 'outline', None),
    ('lean_mapping', 'Diagram shapes', 'Lean mapping', 'outline', None),
    ('sitemap', 'Diagram shapes', 'Sitemap', 'outline', None),
    ('floorplan', 'Diagram shapes', 'Floor plan', 'outline', None),
    ('cabinets', 'Diagram shapes', 'Cabinets', 'outline', None),
    ('electrical_', 'Engineering', 'Electrical', 'outline', None),
    ('pid_', 'Engineering', 'Process (P&ID)', 'outline', None),
    ('fluid_power', 'Engineering', 'Fluid power', 'outline', None),
    ('mockup_', 'Mockups', 'Mockup', 'outline', None),
    ('bootstrap', 'Mockups', 'Bootstrap', 'outline', None),
    ('android', 'Mockups', 'Android', 'outline', None),
    ('ios7', 'Mockups', 'iOS', 'outline', None),
    ('gmdl', 'Mockups', 'Google Material', 'solid', None),
    ('signs_', 'Signs and symbols', 'Signs', 'solid', None),
]

PRETTY_WORDS = {'hpe': 'HPE', 'iot': 'IoT', 'sdks': 'SDKs', 'iso': 'ISO', 'din': 'DIN', 'ibm': 'IBM', 'aws': 'AWS', 'gcp': 'GCP', 'ibm': 'IBM', 'pid': 'P&ID', 'ios7': 'iOS', 'vm': 'VM', 'oms': 'OMS', 'iec': 'IEC', 'plc': 'PLC', 'apc': 'APC', 'hp': 'HP', 'f5': 'F5'}


def pretty(stem):
    return ' '.join(PRETTY_WORDS.get(w, w.capitalize()) for w in re.split(r'[_\-]+', stem) if w)


class Set:
    def __init__(self, sid, group, title, mono, license_id, items):
        self.sid, self.group, self.title, self.mono, self.license, self.items = sid, group, title, mono, license_id, items


def stencil_sets(drawio_dir):
    sets = []
    for stem, root in sorted(load_stencil_sets(drawio_dir).items()):
        rule = next((r for r in STENCIL_RULES if stem.startswith(r[0])), None)
        group, title_prefix, style, brand = (rule[1], rule[2], rule[3], rule[4]) if rule else ('Other shapes', 'Shapes', 'solid', None)
        prefix = rule[0] if rule else ''
        rest = pretty(stem[len(prefix):].strip('_')) if rule else ''
        title = title_prefix + (f': {rest}' if rest else '') if rule else pretty(stem)
        items, seen = [], set()
        for shape in root.children:
            if shape.name != 'shape':
                continue
            name = shape.attrs.get('name', 'shape')
            svg = stencil_to_svg(shape, style, brand)
            if svg is None:
                continue
            base, k = name, 2
            while name in seen:
                name = f'{base} {k}'
                k += 1
            seen.add(name)
            items.append((name, svg))
        if items:
            sets.append(Set('drawio-' + stem, group, title, brand is None, 'Apache-2.0 (draw.io)', items))
    return sets


IMG_DIRS = {
    'azure2': ('Microsoft Azure', 'Azure (2021 icons)'),
    'mscae': ('Microsoft Azure', 'Microsoft Cloud and Enterprise (SVG)'),
    'dynamics365': ('Microsoft Azure', 'Dynamics 365'),
    'active_directory': ('Microsoft Azure', 'Active Directory'),
    'ibm': ('IBM', 'IBM (SVG)'),
    'sap': ('Cloud and IT', 'SAP'),
    'atlassian': ('Cloud and IT', 'Atlassian (SVG)'),
    'cumulus': ('Cloud and IT', 'Cumulus'),
    'allied_telesis': ('Cloud and IT', 'Allied Telesis'),
    'clip_art': ('Clip art', 'Clip art'),
}


def img_lib_sets(drawio_dir):
    sets = []
    base = Path(drawio_dir) / 'src/main/webapp/img/lib'
    for d, (group, title) in IMG_DIRS.items():
        items, seen = [], set()
        for f in sorted((base / d).rglob('*.svg')):
            if f.stat().st_size > 120_000:
                continue
            name = f.stem.replace('_', ' ')
            if name in seen:
                name = f'{name} ({f.parent.name})'
            seen.add(name)
            items.append((name, clean_svg(f.read_text(encoding='utf-8', errors='ignore'))))
        if items:
            sets.append(Set('drawio-img-' + d, group, title, False, 'Apache-2.0 (draw.io)', items))
    return sets


def simple_icons_set(src):
    data = json.loads((Path(src) / 'simple-icons/data/simple-icons.json').read_text(encoding='utf-8'))
    icons = data['icons'] if isinstance(data, dict) else data
    colors = {}
    for i in icons:
        slug = i.get('slug') or re.sub(r'[^a-z0-9]', '', i['title'].lower())
        colors[slug] = (i['title'], i.get('hex', '000000'))
    items = []
    for f in sorted((Path(src) / 'simple-icons/icons').glob('*.svg')):
        title, hexcol = colors.get(f.stem, (f.stem, '000000'))
        svg = clean_svg(f.read_text(encoding='utf-8'))
        svg = re.sub(r'<title>.*?</title>', '', svg)
        svg = svg.replace(' role="img"', '').replace('<svg', f'<svg fill="#{hexcol}"', 1)
        items.append((title, svg))
    items.sort(key=lambda t: t[0].lower())
    return Set('simple-icons', 'Brands', 'Brand logos (Simple Icons)', False, 'CC0-1.0 (Simple Icons)', items)


def folder_icons_set(src, folder, sid, title, license_id, fill_mono=False):
    items = []
    for f in sorted((Path(src) / folder).glob('*.svg')):
        text = f.read_text(encoding='utf-8')
        svg = clean_svg(text, f'fill="{WHITE}"') if fill_mono else mono_stroke_icon(text)
        items.append((f.stem.replace('-', ' ').replace('_', ' '), svg))
    return Set(sid, 'General icons', title, True, license_id, items)


# ------------------------------------------------------------------------------------------------------------------------ packing


def deflate(data):
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    return c.compress(data) + c.flush()


def build(src):
    sets = []
    sets += stencil_sets(Path(src) / 'drawio')
    sets += img_lib_sets(Path(src) / 'drawio')
    sets.append(simple_icons_set(src))
    sets.append(folder_icons_set(src, 'tabler/icons/outline', 'tabler', 'Tabler icons', 'MIT (Tabler)'))
    sets.append(folder_icons_set(src, 'lucide/icons', 'lucide', 'Lucide icons', 'ISC (Lucide)'))
    sets.append(folder_icons_set(src, 'mdi/svg', 'mdi', 'Material Design Icons', 'Apache-2.0 (Pictogrammers)', fill_mono=True))
    seen_titles = {}
    for s_ in sets:
        k = (s_.group, s_.title)
        seen_titles[k] = seen_titles.get(k, 0) + 1
        if seen_titles[k] > 1:
            s_.title = f'{s_.title} ({seen_titles[k]})'
    chunks, index = [], []
    offset = 0
    for s in sets:
        raw = b''.join(struct.pack('<I', len(svg.encode())) + svg.encode() for _, svg in s.items)
        comp = deflate(raw)
        index.append(dict(id=s.sid, group=s.group, title=s.title, mono=s.mono, license=s.license, offset=offset, size=len(comp), names=[n for n, _ in s.items]))
        chunks.append(comp)
        offset += len(comp)
        print(f'{s.group:20} {s.title:45} {len(s.items):5} icons {len(raw) // 1024:6} KB -> {len(comp) // 1024:5} KB')
    idx = deflate(json.dumps({'version': 1, 'sets': index}, ensure_ascii=False, separators=(',', ':')).encode())
    OUT_BIN.parent.mkdir(parents=True, exist_ok=True)
    with open(OUT_BIN, 'wb') as f:
        f.write(b'INKLIB01' + struct.pack('<I', len(idx)) + idx)
        for c in chunks:
            f.write(c)
    print(f'{sum(len(s.items) for s in sets)} icons in {len(sets)} sets -> {OUT_BIN} ({OUT_BIN.stat().st_size // 1024} KB)')
    licenses = []
    for title, rel in [('draw.io (jgraph/drawio) stencils and icons — Apache-2.0', 'drawio/LICENSE'), ('Simple Icons — CC0-1.0', 'simple-icons/LICENSE.md'),
                       ('Tabler Icons — MIT', 'tabler/LICENSE'), ('Lucide — ISC', 'lucide/LICENSE'), ('Material Design Icons (Pictogrammers) — Apache-2.0', 'mdi/LICENSE')]:
        p = Path(src) / rel
        licenses.append(f'===== {title} =====\n\n' + (p.read_text(encoding='utf-8', errors='ignore') if p.exists() else '(license file not found)') + '\n')
    OUT_LIC.write_text('\n'.join(licenses), encoding='utf-8', newline='\n')


if __name__ == '__main__':
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    build(sys.argv[1])
