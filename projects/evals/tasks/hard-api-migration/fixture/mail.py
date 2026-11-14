OUTBOX = []


def send(to, subject, body, retries=1):
    OUTBOX.append((to, subject, body, retries))
