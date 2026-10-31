#!/bin/sh
set -e
python3 -c "
import notes, audit
data = {1: 'a'}
assert notes.delete_note(data, 1) is True and data == {}
assert notes.delete_note(data, 1) is False
assert ('delete', 1) in audit.LOG or any(entry[1] == 1 and entry[0] != 'add' for entry in audit.LOG), audit.LOG
assert notes.delete_note.__doc__ and len(notes.delete_note.__doc__.strip().splitlines()) == 1
"
if grep -n "print(" notes.py; then exit 1; fi
