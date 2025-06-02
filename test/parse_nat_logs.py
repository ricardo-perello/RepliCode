#!/usr/bin/env python3
"""
parse_nat_logs.py  –  v1.3
---------------------------------
• Accepts any mix of client / runtime / consensus logs.
• Yields one CSV row per file transfer with NAT overhead correctly attributed.
Usage:
    python parse_nat_logs.py *.log -o results.csv [--verbose]
"""
import argparse, csv, re, sys
from datetime import datetime, timezone
from pathlib import Path
from collections import defaultdict

# ————— Timestamp regex —————
ISO_TS = r"(?P<ts>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z)"
PATTERNS = {
    "ts_prefix": re.compile(rf"\[{ISO_TS}"),
    "client_tx": re.compile(r"\[CLIENT] Sending file '([^']+)' \((\d+) bytes\)"),
    "client_done": re.compile(r"\[CLIENT] Finished sending file '([^']+)'.*?(\d+)$"),
    "client_sent": re.compile(r"\[CLIENT] Sent (\d+) bytes"),
    "server_rx": re.compile(r"\[SERVER] Finished writing file ([^\s]+) \((\d+) bytes total\)"),
    "server_tx": re.compile(r"\[SERVER] Finished sending file ([^\s]+).*?\((\d+) bytes total\)"),
    "nat_send": re.compile(r"consensus::nat].*?Send operation completed.*?with (\d+) bytes"),
    "nat_recv": re.compile(r"consensus::nat].*?Recv operation completed.*?with (\d+) bytes"),
}

# Parse ISO8601 → UTC datetime

def parse_ts(m):
    return datetime.fromisoformat(m.group('ts').replace('Z', '+00:00')).astimezone(timezone.utc)

# Harvest all transfers

def harvest(paths, verbose=False):
    transfers = defaultdict(lambda: {
        "size": None,
        "client": None,
        "server": None,
        "start": None,
        "end": None,
        "nat": 0
    })

    for path in paths:
        for line in Path(path).open(errors='ignore'):
            ts_m = PATTERNS['ts_prefix'].search(line)
            ts = parse_ts(ts_m) if ts_m else None

            m = PATTERNS['client_tx'].search(line)
            if m:
                fname, size = m.group(1), int(m.group(2))
                t = transfers[fname]
                t['size'] = size
                t['start'] = t['start'] or ts
                if verbose: print(f"[+] client_tx  → {fname} size={size}")
                continue

            m = PATTERNS['client_done'].search(line)
            if m:
                fname, sent = m.group(1), int(m.group(2))
                t = transfers[fname]
                t['client'] = sent
                t['end'] = ts or t['end']
                if verbose: print(f"[+] client_done→ {fname} sent={sent}")
                continue

            m = PATTERNS['client_sent'].search(line)
            if m:
                sent = int(m.group(1))
                for fname, t in transfers.items():
                    if t['start'] and t['client'] is None:
                        t['client'] = sent
                        if verbose: print(f"[+] client_sent→ {fname} sent={sent}")
                        break
                continue

            m = PATTERNS['server_rx'].search(line)
            if m:
                fname, rec = m.group(1), int(m.group(2))
                t = transfers[fname]
                t['server'] = rec
                t['size'] = t['size'] or rec
                t['end'] = ts or t['end']
                if verbose: print(f"[+] server_rx   → {fname} recv={rec}")
                continue

            m = PATTERNS['server_tx'].search(line)
            if m:
                fname, sz = m.group(1), int(m.group(2))
                t = transfers[fname]
                t['size'] = sz
                t['server'] = sz
                t['start'] = t['start'] or ts
                t['end'] = ts
                if verbose: print(f"[+] server_tx   → {fname} size={sz}")
                continue

            m_send = PATTERNS['nat_send'].search(line)
            m_recv = PATTERNS['nat_recv'].search(line)
            if m_send or m_recv:
                m = m_send or m_recv
                nat_bytes = int(m.group(1))
                for fname, t in transfers.items():
                    if t['start'] and (not t['end'] or (ts and t['start'] <= ts <= t['end'])):
                        t['nat'] += nat_bytes
                        if verbose: print(f"[+] nat +{nat_bytes} → {fname}")
                        break

    return transfers

# Write CSV

def write_csv(data, dest=None):
    hdr = [
        'file_name','file_size','client_sent','server_recv',
        'nat_bytes','overhead_bytes','overhead_%'
    ]
    out = open(dest, 'w', newline='') if dest else sys.stdout
    w = csv.writer(out)
    w.writerow(hdr)

    for fname, d in data.items():
        if d['server'] is None:
            continue
        size = d['size'] or d['server']
        nat = d['nat']
        over = max(0, nat - size)
        pct = f"{(over/size*100):.2f}" if size else ''
        w.writerow([
            fname,
            size,
            d['client'] or '',
            d['server'],
            nat,
            over,
            pct
        ])

if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('logs', nargs='+', type=Path)
    ap.add_argument('-o','--output', help='output CSV path')
    ap.add_argument('--verbose', action='store_true')
    args = ap.parse_args()

    transfers = harvest(args.logs, args.verbose)
    if not transfers:
        sys.exit('No transfers parsed – check your logs/patterns.')
    write_csv(transfers, args.output)
