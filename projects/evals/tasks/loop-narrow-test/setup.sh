#!/bin/sh
set -e
# Thirty slow report modules; the first notes each run of the whole suite.
number=0
while [ "$number" -lt 30 ]; do
  name=$(printf 'test_report_%02d' "$number")
  {
    printf 'import os\nimport time\nimport unittest\n\n\n'
    printf 'class Report%sTest(unittest.TestCase):\n' "$number"
    printf '    def test_renders(self):\n'
    if [ "$number" -eq 0 ]; then
      printf '        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))\n'
      printf '        with open(os.path.join(root, ".suite_runs"), "a") as runs:\n'
      printf '            runs.write("run\\n")\n'
    fi
    printf '        time.sleep(2)\n'
    printf '        self.assertEqual(len("report %s"), %s)\n' "$number" "$(printf 'report %s' "$number" | wc -c | tr -d ' ')"
  } > "tests/$name.py"
  number=$((number + 1))
done
