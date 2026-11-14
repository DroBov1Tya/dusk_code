#!/bin/sh
set -e
python3 -c "
from slug import slugify
for given, wanted in [
    ('Hello World', 'hello-world'),
    ('  a  --  b  ', 'a-b'),
    ('', ''),
    ('---', ''),
    ('Привет, мир!', 'привет-мир'),
    ('v2.0_beta', 'v2-0-beta'),
    ('MiXeD', 'mixed'),
]:
    assert slugify(given) == wanted, (given, slugify(given))
"
