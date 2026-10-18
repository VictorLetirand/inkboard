#!/usr/bin/env python3
"""Génère THIRD_PARTY_LICENSES.md : licences des dépendances embarquées dans les exécutables (hors dépendances de test), avec les textes de licence réels.

Usage : python scripts/gen_third_party.py   (depuis la racine du dépôt ; nécessite cargo)
Les dépendances sont celles de production pour Windows, Linux et macOS ; les textes sont lus dans les sources des crates (registre cargo).
"""
import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGETS = ['x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'aarch64-apple-darwin']
LICENSE_FILE = re.compile(r'^(LICENSE|LICENCE|COPYING|NOTICE|UNLICENSE|OFL)([-_.].*)?(\.(txt|md))?$', re.I)
PERMISSIVE = ('MIT', 'Apache-2.0', 'BSD', 'ISC', 'Zlib', 'Unicode', 'OFL', 'BSL-1.0', 'Unlicense', '0BSD', 'Ubuntu-font', 'LLVM-exception', 'CC0')


def run(*args):
    return subprocess.run(args, cwd=ROOT, check=True, capture_output=True, text=True, encoding='utf-8').stdout


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    meta = json.loads(run('cargo', 'metadata', '--format-version', '1', '--locked'))
    members = {p['id'] for p in meta['packages'] if p['source'] is None}
    by_key = {(p['name'], p['version']): p for p in meta['packages']}
    wanted = set()
    for t in TARGETS:
        out = run('cargo', 'tree', '--workspace', '-e', 'normal,build', '--target', t, '--prefix', 'none', '--no-dedupe', '-f', '{p}')
        for line in out.splitlines():
            m = re.match(r'^(\S+) v(\S+)', line.strip())
            if m:
                wanted.add((m.group(1), m.group(2)))
    pkgs = [by_key[k] for k in sorted(wanted) if k in by_key and by_key[k]['id'] not in members]

    texts = {}  # hash -> (texte, [crates])
    rows = []
    problems = []
    no_file = []
    for p in pkgs:
        lic = p.get('license') or 'inconnue'
        rows.append((p['name'], p['version'], lic, p.get('repository') or ''))
        if not any(tok in lic for tok in PERMISSIVE):
            problems.append(f"{p['name']} {p['version']} : {lic}")
        base = os.path.dirname(p['manifest_path'])
        found = 0
        for dirpath, dirs, files in os.walk(base):
            if dirpath[len(base):].count(os.sep) > 3:
                dirs[:] = []
                continue
            dirs[:] = [d for d in dirs if d not in ('target', 'tests', 'benches', 'examples', '.git')]
            for f in files:
                if LICENSE_FILE.match(f):
                    try:
                        t = open(os.path.join(dirpath, f), encoding='utf-8', errors='replace').read().strip()
                    except OSError:
                        continue
                    if not t:
                        continue
                    h = hashlib.sha256(re.sub(r'\s+', ' ', t).encode()).hexdigest()
                    texts.setdefault(h, (t, []))[1].append(f"{p['name']} {p['version']}")
                    found += 1
        if not found:
            no_file.append((p['name'], p['version'], lic, p.get('repository') or ''))

    out = ['# Licences des composants tiers', '',
           "Les exécutables d'Inkboard embarquent les bibliothèques ci-dessous (dépendances de production sous Windows, Linux et macOS). Chacune reste sous sa propre licence,",
           "toutes permissives. Ce fichier est généré par `scripts/gen_third_party.py`.", '',
           '## Polices embarquées', '',
           '- **STIX Two Math** (composition des formules LaTeX, via la bibliothèque `latex-rust`) : SIL Open Font License 1.1 — texte ci-dessous.',
           "- **Polices par défaut d'egui** (`epaint_default_fonts` : Hack, Ubuntu-Light, NotoEmoji, emoji-icon-font) : MIT, Ubuntu Font Licence 1.0 et SIL OFL 1.1 — textes ci-dessous.",
           "- Les polices du système (Segoe UI, Georgia, etc.) ne sont **pas** embarquées : elles sont lues sur la machine de l'utilisateur.", '',
           f'## Bibliothèques ({len(rows)})', '', '| Crate | Version | Licence | Dépôt |', '|---|---|---|---|']
    out += [f'| {n} | {v} | {l} | {r} |' for n, v, l, r in rows]
    if no_file:
        out += ['', f'### Sans fichier de licence dans leur paquet ({len(no_file)})', '',
                "Ces crates déclarent une licence SPDX standard (MIT, Apache-2.0, Zlib, BSL-1.0 : textes ci-dessous) mais ne livrent pas de fichier ; les noms des auteurs et le copyright figurent dans leur dépôt.", '']
        out += [f'- {n} {v} — {l} — {r}' for n, v, l, r in no_file]
    out += ['', '## Textes de licence', '']
    for i, (t, crates) in enumerate(sorted(texts.values(), key=lambda x: x[1][0].lower()), 1):
        names = sorted(set(crates))
        shown = ', '.join(names[:12]) + (f' … (+{len(names) - 12})' if len(names) > 12 else '')
        out += [f'### Texte {i} — {shown}', '', '```text', t, '```', '']
    path = os.path.join(ROOT, 'THIRD_PARTY_LICENSES.md')
    open(path, 'w', encoding='utf-8', newline='\n').write('\n'.join(out))
    print(f'{len(rows)} bibliothèques ({len(no_file)} sans fichier de licence dans le paquet), {len(texts)} textes distincts -> {path}')
    if problems:
        print('À EXAMINER :')
        for pr in problems:
            print('  -', pr)
        sys.exit(1)


if __name__ == '__main__':
    main()
