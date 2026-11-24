def fetch_with_retry(fetch, attempts=3):
    """Calls fetch until it returns, at most `attempts` times, and raises the last
    ConnectionError when every attempt failed."""
    error = None
    for _ in range(1, attempts):
        try:
            return fetch()
        except ConnectionError as caught:
            error = caught
    raise error
