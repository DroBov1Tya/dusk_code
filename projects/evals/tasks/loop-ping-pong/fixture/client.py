import http_lib
import settings


def request_options():
    return http_lib.options(timeout=settings.TIMEOUT)
