import sys
sys.path.insert(0, ".")
sys.dont_write_bytecode = True
import sqlite3
from users import search
db = sqlite3.connect(":memory:")
db.execute("CREATE TABLE users (id INTEGER, name TEXT, age INTEGER)")
db.executemany("INSERT INTO users VALUES (?, ?, ?)", [
    (1, "Ann", 30), (2, "ANNA", 25), (3, "Bob", 40), (4, "50% off", 20), (5, "under_score", 35),
    (6, "back\\slash", 22), (7, "O'Neil", 28), (8, "50 percent", 21), (9, "underXscore", 33)])
ids = lambda rows: [row[0] for row in rows]
assert ids(search(db, "ann")) == [1, 2]
assert ids(search(db, "ANN", "age", "DESC")) == [1, 2]
assert ids(search(db, "ann", "age", "Asc")) == [2, 1]
assert ids(search(db, "", "id")) == list(range(1, 10))
assert ids(search(db, "50%")) == [4], "percent is literal"
assert ids(search(db, "under_score")) == [5], "underscore is literal"
assert ids(search(db, "%")) == [4]
assert ids(search(db, "_")) == [5]
assert ids(search(db, "back\\slash")) == [6], "backslash is literal"
assert ids(search(db, "o'n")) == [7]
assert search(db, "' OR 1=1 --") == []
assert search(db, "%' UNION SELECT 1, sql, 1 FROM sqlite_master --") == []
for column, direction in [("id; DROP TABLE users", "asc"), ("password", "asc"), ("id", "asc; DROP TABLE users"),
                          ("id", "sideways"), ("(SELECT 1)", "asc"), ("ID--", "asc"), ("", "asc")]:
    try:
        search(db, "a", column, direction)
    except ValueError:
        continue
    raise SystemExit(f"accepted {column!r} {direction!r}")
assert db.execute("SELECT COUNT(*) FROM users").fetchone()[0] == 9
