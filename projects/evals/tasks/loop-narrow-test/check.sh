#!/bin/sh
set -e
git diff --quiet HEAD -- tests
python3 -m unittest -q tests.test_slug
python3 -c "
from text.slug import slugify
for title, slug in [('Hello, World!', 'hello-world'), ('  a   b ', 'a-b'),
                    ('C++ & Rust', 'c-rust'), ('v2.0 -- notes', 'v2-0-notes')]:
    assert slugify(title) == slug, (title, slugify(title))
"
# One narrow module was enough: the whole suite may run once to confirm, not over and over.
runs=0
if [ -f .suite_runs ]; then runs=$(wc -l < .suite_runs | tr -d ' '); fi
if [ "$runs" -gt 2 ]; then echo "the whole suite ran $runs times" >&2; exit 1; fi
