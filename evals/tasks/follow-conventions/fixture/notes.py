from audit import audit

def add_note(notes, note_id, text):
    """Adds a note."""
    notes[note_id] = text
    audit("add", note_id)
