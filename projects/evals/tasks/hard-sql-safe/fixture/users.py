def search(db, text, order_by="id", direction="asc"):
    query = (
        f"SELECT id, name, age FROM users WHERE name LIKE '%{text}%' "
        f"ORDER BY {order_by} {direction}"
    )
    return db.execute(query).fetchall()
