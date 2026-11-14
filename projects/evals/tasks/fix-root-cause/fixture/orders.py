from money import percent

def total(amount, discount):
    return round(amount - percent(amount, discount), 2)
