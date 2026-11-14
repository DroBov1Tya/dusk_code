#!/bin/sh
set -e
cat > bank.py <<'PY'
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
        ordered = [self.accounts[name] for name in sorted({source, target})]
        for account in ordered:
            account.lock.acquire()
        try:
            if one.balance < amount:
                return False
            one.balance -= amount
            other.balance += amount
            return True
        finally:
            for account in reversed(ordered):
                account.lock.release()

    def total(self):
        ordered = [self.accounts[name] for name in sorted(self.accounts)]
        for account in ordered:
            account.lock.acquire()
        try:
            return sum(account.balance for account in ordered)
        finally:
            for account in reversed(ordered):
                account.lock.release()
PY
