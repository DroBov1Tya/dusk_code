#!/bin/sh
set -e
python3 - <<'PY'
import os
os.mkdir("senders")
open("senders/__init__.py", "w").close()
for number in range(30):
    retries = number % 3 + 1
    to, subject, body = f'"u{number}@x"', f'"s{number}"', f'"b{number}"'
    kind = number % 6
    head = 'from mail import send\n\n\ndef run():\n'
    if kind == 0:
        text = f'    send({to}, {subject}, {body}, {retries})\n'
    elif kind == 1:
        text = f'    send(body={body}, to={to}, subject={subject}, retries={retries})\n'
    elif kind == 2:
        text = f'    arguments = dict(to={to}, subject={subject}, body={body}, retries={retries})\n    send(**arguments)\n'
    elif kind == 3:
        head = 'import mail\n\n\ndef run():\n'
        text = f'    getattr(mail, "send")({to}, {subject}, {body}, {retries})\n'
    elif kind == 4:
        head = 'from functools import partial\n\nfrom mail import send\n\n\ndef run():\n'
        text = f'    notify = partial(send, {to}, retries={retries})\n    notify({subject}, {body})\n'
    else:
        text = f'    for subject in [{subject}, "again"]:\n        send({to}, subject, {body}, {retries})\n'
    open(f"senders/m{number}.py", "w").write(head + text)
PY
