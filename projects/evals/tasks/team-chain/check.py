"""The rules of task.md the visible tests leave out, layer by layer."""

import os
import sqlite3
import subprocess
import sys
import tempfile

sys.path.insert(0, ".")

FOLDER = tempfile.mkdtemp()


def raises(kind, function, *arguments):
    try:
        function(*arguments)
    except kind:
        return True
    return False


def schema():
    from inventory.schema import migrate

    conn = sqlite3.connect(":memory:")
    migrate(conn)
    insert = "INSERT INTO items (sku, name, quantity) VALUES (?, ?, ?)"
    assert raises(sqlite3.IntegrityError, conn.execute, insert, ("A", "a", -1))
    assert raises(sqlite3.IntegrityError, conn.execute, insert, ("A", None, 1))
    conn.execute(insert, ("A", "a", 1))
    moved = "INSERT INTO moves (sku, delta) VALUES ('A', 0)"
    assert raises(sqlite3.IntegrityError, conn.execute, moved)
    conn.execute("INSERT INTO moves (sku, delta) VALUES ('A', 3)")
    assert conn.execute("SELECT note FROM moves").fetchone() == ("",)
    path = os.path.join(FOLDER, "newer.db")
    newer = sqlite3.connect(path)
    newer.execute("PRAGMA user_version = 2")
    newer.commit()
    assert raises(RuntimeError, migrate, newer)
    tables = newer.execute("SELECT count(*) FROM sqlite_master").fetchone()[0]
    assert tables == 0, "a newer database was changed"
    committed = os.path.join(FOLDER, "committed.db")
    migrate(sqlite3.connect(committed))
    assert sqlite3.connect(committed).execute("PRAGMA user_version").fetchone() == (1,)


def api_layer():
    from inventory import api

    path = os.path.join(FOLDER, "api.db")
    conn = api.open_store(path)
    for sku in ["ABC-123", "ABCD-1234", "abc-1234", "ABC-1234 ", " ABC-1234", "AB1-1234"]:
        assert raises(ValueError, api.add_item, conn, sku, "x"), sku
    assert raises(ValueError, api.add_item, conn, "ABC-1234", "   ")
    api.add_item(conn, "ZZZ-0001", "last")
    api.add_item(conn, "ABC-1234", "first")
    assert raises(ValueError, api.add_item, conn, "ABC-1234", "again")
    assert raises(KeyError, api.move, conn, "XYZ-0000", 1)
    assert raises(KeyError, api.history, conn, "XYZ-0000")
    assert raises(ValueError, api.move, conn, "ABC-1234", 0)
    api.move(conn, "ABC-1234", 4, "in")
    assert raises(ValueError, api.move, conn, "ABC-1234", -5)
    assert api.history(conn, "ABC-1234") == [(4, "in")], "a refused move was recorded"
    assert [row[0] for row in api.stock(conn)] == ["ABC-1234", "ZZZ-0001"]
    raw = sqlite3.connect(path)
    assert raw.execute("SELECT quantity FROM items WHERE sku = 'ABC-1234'").fetchone() == (4,)
    assert raw.execute("SELECT count(*) FROM moves").fetchone() == (1,), "not committed"
    assert conn.execute("PRAGMA foreign_keys").fetchone() == (1,)


def cli():
    db = os.path.join(FOLDER, "cli.db")

    def run(*arguments):
        command = [sys.executable, "-m", "inventory", db, *arguments]
        return subprocess.run(command, capture_output=True, text=True)

    assert run("stock").stdout == "no items\n"
    run("add", "BBB-0002", "nut")
    run("add", "AAA-0001", "washer")
    run("move", "AAA-0001", "120")
    run("move", "BBB-0002", "7", "box")
    run("move", "BBB-0002", "-3")
    assert run("stock").stdout == "AAA-0001  120  washer\nBBB-0002    4  nut\n"
    assert run("history", "BBB-0002").stdout == "+7 box\n-3\n"
    for wrong in [("move", "BBB-0002", "-9"), ("move", "BBB-0002", "x"), ("dance",),
                  ("add", "bad", "x"), ("history", "CCC-0003"), ("add", "CCC-0003")]:
        done = run(*wrong)
        assert done.returncode == 2 and done.stdout == "", wrong
        assert done.stderr.startswith("error: ") and done.stderr.count("\n") == 1, wrong


failed = []
for layer in (schema, api_layer, cli):
    try:
        layer()
    except Exception as error:
        failed.append(f"{layer.__name__}: {type(error).__name__} {error}")
if failed:
    sys.exit("hidden cases fail: " + "; ".join(failed))
