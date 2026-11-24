from retry import fetch_with_retry


def sync(service):
    """The items on the server, fetched with retries: the server drops requests."""
    return fetch_with_retry(service.get)["items"]
