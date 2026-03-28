#!/usr/bin/env python3
"""Inject random L2-normalized 512-dim embeddings into riffgrep's index.db.

Usage:
    python3 scripts/inject_synthetic_embeddings.py [db_path]

Default db_path: ~/Library/Application Support/riffgrep/index.db
"""

import sqlite3
import struct
import sys
import os
import random
import math

def normalize(vec):
    norm = math.sqrt(sum(x * x for x in vec))
    if norm == 0:
        return vec
    return [x / norm for x in vec]

def main():
    default_db = os.path.expanduser("~/Library/Application Support/riffgrep/index.db")
    db_path = sys.argv[1] if len(sys.argv) > 1 else default_db

    conn = sqlite3.connect(db_path)
    rows = conn.execute("SELECT id, path FROM samples WHERE embedding IS NULL").fetchall()

    if not rows:
        print("All files already have embeddings.")
        return

    count = 0
    for row_id, path in rows:
        vec = normalize([random.gauss(0, 1) for _ in range(512)])
        blob = struct.pack(f"<{len(vec)}f", *vec)
        conn.execute("UPDATE samples SET embedding = ? WHERE id = ?", (blob, row_id))
        count += 1

    conn.commit()
    conn.close()
    print(f"Injected {count} synthetic embeddings into {db_path}")

if __name__ == "__main__":
    main()
