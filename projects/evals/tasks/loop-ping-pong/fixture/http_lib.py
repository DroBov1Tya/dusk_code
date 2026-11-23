"""The parts of the HTTP library this project uses."""


def options(timeout):
    """Request options. `timeout` is in seconds, as a number."""
    return {"timeout": timeout}
