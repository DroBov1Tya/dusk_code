from money import percent

def with_tax(amount, rate):
    return round(amount + percent(amount, rate), 2)
