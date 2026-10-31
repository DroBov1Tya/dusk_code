import threading


class Account:
    def __init__(self, balance):
        self.balance = balance
        self.lock = threading.Lock()


class Bank:
    def __init__(self, balances):
        self.accounts = {name: Account(amount) for name, amount in balances.items()}

    def transfer(self, source, target, amount):
        """Moves money; False when the source cannot pay."""
        one, other = self.accounts[source], self.accounts[target]
        with one.lock:
            with other.lock:
                if one.balance < amount:
                    return False
                one.balance -= amount
                other.balance += amount
                return True

    def total(self):
        return sum(account.balance for account in self.accounts.values())
