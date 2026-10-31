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
    sent = mail.OUTBOX[0]
    message, retries = (sent[0], sent[1]) if len(sent) == 2 else (None, None)
    assert message is not None, (number, sent)
    assert (message.to, message.subject, message.body) == (f"u{number}@x", f"s{number}", f"b{number}"), (number, message)
    assert retries == (number % 3) + 1, (number, retries)
