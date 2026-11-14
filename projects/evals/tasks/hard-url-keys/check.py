import sys
sys.path.insert(0, ".")
sys.dont_write_bytecode = True
from urls import cache_key as key
same = [("https://site.org/docs/", "https://site.org/docs"), ("https://site.org/", "https://site.org"),
        ("https://site.org/docs/?x=1", "https://site.org/docs?x=1"), ("HTTPS://SITE.org/docs/", "https://site.org/docs"),
        ("https://site.org/?x=1", "https://site.org?x=1")]
for one, other in same:
    assert key(one) == key(other), (one, other, key(one), key(other))
differ = [("https://site.org/a", "https://site.org/A"), ("https://site.org/a?x=1", "https://site.org/a?x=2"),
          ("https://site.org/a/b", "https://site.org/a"), ("https://site.org/a?next=/b/", "https://site.org/a?next=/b"),
          ("http://site.org/a", "https://site.org/a")]
for one, other in differ:
    assert key(one) != key(other), (one, other)
