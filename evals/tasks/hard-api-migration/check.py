import sys
sys.path.insert(0, ".")
sys.dont_write_bytecode = True
import importlib, inspect, re, subprocess
import mail
assert list(inspect.signature(mail.send).parameters) == ["message", "retries"], inspect.signature(mail.send)
assert mail.Message("a", "b", "c").subject == "b"
for number in range(30):
    module = importlib.import_module(f"senders.m{number}")
    mail.OUTBOX.clear()
    module.run()
    assert len(mail.OUTBOX) == 1 + (number % 6 == 5), (number, mail.OUTBOX)
    # The task does not say how OUTBOX records a mail, so any plain record will do.
    sent = mail.OUTBOX[0]
    if len(sent) == 2:
        message, retries = sent
        fields = (message.to, message.subject, message.body, retries)
    else:
        fields = tuple(sent)
    assert fields == (f"u{number}@x", f"s{number}", f"b{number}", number % 3 + 1), (number, sent)
