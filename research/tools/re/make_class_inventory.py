"""Renders re/rtti_inventory.tsv (from RttiInventory.java) as docs/class-inventory.md, grouped by engine area.
usage: python -I tools/re/make_class_inventory.py <re dir> <docs dir>"""
import csv
import os
import sys

re_dir, docs_dir = sys.argv[1], sys.argv[2]
rows = list(csv.DictReader(open(os.path.join(re_dir, 'rtti_inventory.tsv'), encoding='utf-8'), delimiter='\t'))

GROUPS = [
    ('Script commands (`ADV::*`)', lambda c: c.startswith('shin::ADV::')),
    ('Layers and layer loaders', lambda c: 'Layer' in c and 'MessageLayer' not in c),
    ('Transitions (wipers)', lambda c: 'Wiper' in c),
    ('Message window and text', lambda c: 'Message' in c or 'TextLayouter' in c or 'TextParser' in c),
    ('Menus and modes', lambda c: any(k in c for k in ('Mode', 'Menu', 'Title', 'Save', 'Load', 'Select', 'Viewer',
                                                      'FirstBoot', 'Browser'))),
    ('Audio and video', lambda c: any(k in c for k in ('Audio', 'Sound', 'BGM', 'Voice', 'Movie'))),
    ('Data, files and assets', lambda c: any(k in c for k in ('Data', 'File', 'Media', 'Asset', 'TXA', 'Font',
                                                              'Texture', 'DepthStencil'))),
    ('Everything else', lambda c: True),
]

placed, sections = set(), []
for title, match in GROUPS:
    members = [r for r in rows if r['class'] not in placed and match(r['class'])]
    placed.update(r['class'] for r in members)
    sections.append((title, sorted(members, key=lambda r: r['class'])))

lines = [
    '# Class inventory (generated)',
    '',
    'Every C++ class with RTTI in the v1.0.3 executable, generated from `re/rtti_inventory.tsv` by',
    '`tools/re/make_class_inventory.py`. Do not edit by hand; regenerate after re-running `RttiInventory.java`.',
    '',
    f'{len(rows)} classes. Addresses are for Ghidra\'s Switch loader (base `0x7100000000`). "Methods" counts the',
    'entries of the primary vtable; `re/symbols.csv` names the ones unique to a class.',
    '',
]
for title, members in sections:
    if not members:
        continue
    lines += [f'## {title} ({len(members)})', '', '| Class | typeinfo | vtable(s) | Methods |', '|---|---|---|---|']
    for r in members:
        vtables = ', '.join(f'`{v}`' for v in filter(None, r['vtables'].split(','))) or '—'
        count = len(list(filter(None, r['virtual_methods'].split(','))))
        lines.append(f"| `{r['class']}` | `{r['typeinfo']}` | {vtables} | {count} |")
    lines.append('')

with open(os.path.join(docs_dir, 'class-inventory.md'), 'w', encoding='utf-8') as f:
    f.write('\n'.join(lines))
print(f'{len(rows)} classes in {sum(1 for _, m in sections if m)} sections')
