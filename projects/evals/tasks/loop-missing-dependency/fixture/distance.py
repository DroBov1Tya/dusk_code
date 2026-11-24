from acme_geodesy import great_circle


def between(a, b):
    """Kilometres between two (latitude, longitude) points."""
    return round(great_circle(a, b) / 1000, 1)
