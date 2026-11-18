#!/bin/sh
set -e
cat > inventory/schema.py <<'PY'
VERSION = 1

TABLES = """
CREATE TABLE IF NOT EXISTS items (
    sku TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    quantity INTEGER NOT NULL DEFAULT 0 CHECK (quantity >= 0)
);
CREATE TABLE IF NOT EXISTS moves (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sku TEXT NOT NULL REFERENCES items(sku),
    delta INTEGER NOT NULL CHECK (delta != 0),
    note TEXT NOT NULL DEFAULT ''
);
"""


def migrate(conn):
    version = conn.execute("PRAGMA user_version").fetchone()[0]
    if version > VERSION:
        raise RuntimeError(f"the database is of a newer schema, {version}")
    if version == VERSION:
        return
    conn.executescript(TABLES + f"PRAGMA user_version = {VERSION};")
    conn.commit()
PY
cat > inventory/api.py <<'PY'
import re
import sqlite3

from inventory.schema import migrate

SKU = re.compile(r"[A-Z]{3}-[0-9]{4}")


def open_store(path):
    conn = sqlite3.connect(path)
    conn.execute("PRAGMA foreign_keys = ON")
    migrate(conn)
    return conn


def add_item(conn, sku, name):
    if not isinstance(sku, str) or not SKU.fullmatch(sku):
        raise ValueError(f"bad SKU {sku!r}")
    if not name.strip():
        raise ValueError("the name is empty")
    try:
        with conn:
            conn.execute("INSERT INTO items (sku, name) VALUES (?, ?)", (sku, name.strip()))
    except sqlite3.IntegrityError:
        raise ValueError(f"{sku} exists") from None


def _quantity(conn, sku):
    row = conn.execute("SELECT quantity FROM items WHERE sku = ?", (sku,)).fetchone()
    if row is None:
        raise KeyError(sku)
    return row[0]


def move(conn, sku, delta, note=""):
    if not isinstance(delta, int) or isinstance(delta, bool) or delta == 0:
        raise ValueError("a move is a non-zero whole number")
    quantity = _quantity(conn, sku) + delta
    if quantity < 0:
        raise ValueError(f"{sku} would go below zero")
    with conn:
        conn.execute("UPDATE items SET quantity = ? WHERE sku = ?", (quantity, sku))
        conn.execute("INSERT INTO moves (sku, delta, note) VALUES (?, ?, ?)", (sku, delta, note))
    return quantity


def stock(conn):
    return conn.execute("SELECT sku, name, quantity FROM items ORDER BY sku").fetchall()


def history(conn, sku):
    _quantity(conn, sku)
    rows = conn.execute("SELECT delta, note FROM moves WHERE sku = ? ORDER BY id", (sku,))
    return rows.fetchall()
PY
cat > inventory/cli.py <<'PY'
import sys

from inventory import api


def run(conn, command, rest):
    if command == "add" and len(rest) >= 2:
        api.add_item(conn, rest[0], " ".join(rest[1:]))
        return [f"added {rest[0]}"]
    if command == "move" and len(rest) >= 2:
        try:
            delta = int(rest[1])
        except ValueError:
            raise ValueError(f"not a number: {rest[1]}") from None
        return [f"{rest[0]}: {api.move(conn, rest[0], delta, ' '.join(rest[2:]))}"]
    if command == "stock" and not rest:
        items = api.stock(conn)
        width = max((len(str(quantity)) for _, _, quantity in items), default=1)
        lines = [f"{sku}  {quantity:>{width}}  {name}" for sku, name, quantity in items]
        return lines or ["no items"]
    if command == "history" and len(rest) == 1:
        moves = api.history(conn, rest[0])
        return [f"{delta:+d} {note}".rstrip() if note else f"{delta:+d}" for delta, note in moves]
    raise ValueError("usage: DB add SKU NAME... | move SKU DELTA [NOTE...] | stock | history SKU")


def main(arguments):
    if len(arguments) < 2:
        print("error: usage: DB COMMAND ...", file=sys.stderr)
        return 2
    try:
        conn = api.open_store(arguments[0])
        try:
            lines = run(conn, arguments[1], arguments[2:])
        finally:
            conn.close()
    except (ValueError, KeyError, RuntimeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    for line in lines:
        print(line)
    return 0
PY
