#!/bin/sh
set -e
python3 - <<'PY'
text = open('report.py').read().replace("""    result = counts(sys.argv[1])
    for key, value in result.items():
        print(f"{key}: {value}")
""", """    arguments = sys.argv[1:]
    as_json = "--json" in arguments
    paths = [argument for argument in arguments if argument != "--json"]
    result = counts(paths[0])
    if as_json:
        print(json.dumps(result))
        return
    for key, value in result.items():
        print(f"{key}: {value}")
""").replace("import sys", "import json\nimport sys")
open('report.py', 'w').write(text)
PY
