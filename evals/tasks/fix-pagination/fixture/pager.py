def page(items, number, size):
    """The items on page `number`, counted from 1."""
    start = (number - 1) * size
    return items[start:start + size - 1]
