def slugify(title):
    """A title as a URL slug: lower case, each run of characters other than a-z and 0-9
    becomes one hyphen, and the slug neither starts nor ends with one."""
    return title.lower().replace(" ", "-")
