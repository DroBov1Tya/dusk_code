from urllib.parse import urlsplit


def cache_key(url):
    """The key a page is cached under. Hosts and schemes are case-insensitive; paths and
    queries are not: /A and /a are different pages, and so are ?x=1 and ?x=2."""
    parts = urlsplit(url)
    rebuilt = f"{parts.scheme.lower()}://{parts.netloc.lower()}{parts.path}"
    if parts.query:
        rebuilt += f"?{parts.query}"
    return rebuilt
