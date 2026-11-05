"""Builds re/symbols.csv (address,name,comment) for ApplySymbols.java from:
  - re/rtti_inventory.tsv (output of RttiInventory.java): typeinfo + vtable labels for every RTTI class, and
    names for virtual methods that appear in exactly one class's primary vtable (shared ones are base-class methods
    whose owner can't be told from the vtables alone, so they stay unnamed);
  - re/manual_symbols.csv: hand-identified functions and constants (these win over generated names).
usage: python -I tools/re/make_symbols.py <re dir>"""
import csv
import os
import sys
from collections import Counter

re_dir = sys.argv[1]
rows = list(csv.DictReader(open(os.path.join(re_dir, 'rtti_inventory.tsv'), encoding='utf-8'), delimiter='\t'))

symbols = {}
method_owners = Counter()
for row in rows:
    for method in filter(None, row['virtual_methods'].split(',')):
        method_owners[method] += 1

for row in rows:
    cls = row['class']
    symbols[row['typeinfo']] = (f'{cls}::typeinfo', row['mangled'])
    for i, vtable in enumerate(filter(None, row['vtables'].split(','))):
        symbols[vtable] = (f'{cls}::vtable' + (f'_{i}' if i else ''), 'primary' if i == 0 else 'secondary vtable')
    for slot, method in enumerate(filter(None, row['virtual_methods'].split(','))):
        if method_owners[method] == 1:
            symbols.setdefault(method, (f'{cls}::vfunc_{slot}', f'slot {slot} of the primary vtable'))

with open(os.path.join(re_dir, 'manual_symbols.csv'), encoding='utf-8') as f:
    for row in csv.DictReader(f):
        symbols[row['address']] = (row['name'], row['comment'])

with open(os.path.join(re_dir, 'symbols.csv'), 'w', newline='', encoding='utf-8') as f:
    writer = csv.writer(f)
    writer.writerow(['address', 'name', 'comment'])
    for address in sorted(symbols, key=lambda a: int(a, 16)):
        writer.writerow([address, *symbols[address]])
print(f'{len(symbols)} symbols written')
