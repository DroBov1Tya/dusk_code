#!/bin/sh
set -e
cat > users.py <<'PY'
COLUMNS = {"id", "name", "age"}
DIRECTIONS = {"asc", "desc"}


def search(db, text, order_by="id", direction="asc"):
    direction = direction.lower()
    if order_by not in COLUMNS or direction not in DIRECTIONS:
        raise ValueError("cannot sort that way")
    pattern = text.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")
    query = (
        "SELECT id, name, age FROM users WHERE name LIKE ? ESCAPE '\\' "
        f"ORDER BY {order_by} {direction}"
    )
    return db.execute(query, (f"%{pattern}%",)).fetchall()
PY
