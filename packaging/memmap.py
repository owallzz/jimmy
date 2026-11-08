#!/usr/bin/env python3
"""Group a process's memory by mapping: memmap.py PID [top-N]
Rss = resident; Priv = private (clean+dirty, roughly what System Monitor shows); Anon = anonymous."""
import sys, re, collections
pid = sys.argv[1]; top = int(sys.argv[2]) if len(sys.argv) > 2 else 12
g = collections.defaultdict(lambda: collections.Counter())
name = None
for line in open(f"/proc/{pid}/smaps"):
    if re.match(r"^[0-9a-f]+-[0-9a-f]+ ", line):
        parts = line.split()
        name = parts[5] if len(parts) > 5 else "[anon]"
        name = name.split("/")[-1] if "/" in name else name
        continue
    k, _, v = line.partition(":")
    if k in ("Rss", "Private_Clean", "Private_Dirty", "Anonymous", "Shared_Clean"):
        g[name][k] += int(v.split()[0])
tot = collections.Counter()
for c in g.values(): tot.update(c)
mb = lambda kb: f"{kb/1024:7.1f}"
priv = lambda c: c["Private_Clean"] + c["Private_Dirty"]
print(f"TOTAL  Rss {mb(tot['Rss'])}  Priv {mb(priv(tot))}  Anon {mb(tot['Anonymous'])}  SharedClean {mb(tot['Shared_Clean'])} MB")
print(f"{'mapping':40} {'Rss':>7} {'Priv':>7} {'Anon':>7}")
for n, c in sorted(g.items(), key=lambda kv: -priv(kv[1]))[:top]:
    print(f"{n[:40]:40} {mb(c['Rss'])} {mb(priv(c))} {mb(c['Anonymous'])}")
