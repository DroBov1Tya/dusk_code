#!/bin/sh
set -e
cat >> notes.py <<'PY'

def delete_note(notes, note_id):
    """Removes a note and says whether it was there."""
    if note_id not in notes:
        return False
    del notes[note_id]
    audit("delete", note_id)
    return True
PY
