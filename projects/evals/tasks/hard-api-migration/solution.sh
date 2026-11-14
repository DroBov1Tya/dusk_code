#!/bin/sh
set -e
python3 - <<'PY'
import re
text = open('mail.py').read()
open('mail.py', 'w').write("""from dataclasses import dataclass

OUTBOX = []


@dataclass
class Message:
    to: str
    subject: str
    body: str


def send(message, retries=1):
    OUTBOX.append((message, retries))
""")
for number in range(30):
    path = f'senders/m{number}.py'
    retries = number % 3 + 1
    call = f'Message("u{number}@x", "s{number}", "b{number}")'
    kind = number % 6
    head = 'from mail import Message, send\n\n\ndef run():\n'
    if kind == 0:
        body = f'    send({call}, {retries})\n'
    elif kind == 1:
        body = f'    send({call}, retries={retries})\n'
    elif kind == 2:
        body = f'    arguments = dict(retries={retries})\n    send({call}, **arguments)\n'
    elif kind == 3:
        head = 'import mail\nfrom mail import Message\n\n\ndef run():\n'
        body = f'    getattr(mail, "send")({call}, {retries})\n'
    elif kind == 4:
        head = 'from functools import partial\n\nfrom mail import Message, send\n\n\ndef run():\n'
        body = f'    notify = partial(send, retries={retries})\n    notify({call})\n'
    else:
        body = f'    for message in [{call}, Message("u{number}@x", "again", "b{number}")]:\n        send(message, {retries})\n'
    open(path, 'w').write(head + body)
PY
