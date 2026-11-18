def open_store(path):
    raise NotImplementedError


def add_item(conn, sku, name):
    raise NotImplementedError


def move(conn, sku, delta, note=""):
    raise NotImplementedError


def stock(conn):
    raise NotImplementedError


def history(conn, sku):
    raise NotImplementedError
