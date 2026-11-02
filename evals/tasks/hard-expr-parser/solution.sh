#!/bin/sh
set -e
cat > calc.py <<'PY'
import re

TOKEN = re.compile(r"\s*(\d+\.\d*|\.\d+|\d+|\*\*|[-+*/%()])")


def tokenize(text):
    tokens, at = [], 0
    text = text.rstrip()
    while at < len(text):
        match = TOKEN.match(text, at)
        if not match:
            raise ValueError("bad character")
        tokens.append(match.group(1))
        at = match.end()
    return tokens


class Parser:
    def __init__(self, tokens):
        self.tokens, self.at = tokens, 0

    def peek(self):
        return self.tokens[self.at] if self.at < len(self.tokens) else None

    def take(self):
        token = self.peek()
        self.at += 1
        return token

    def sum(self):
        value = self.term()
        while self.peek() in ("+", "-"):
            value = value + self.term() if self.take() == "+" else value - self.term()
        return value

    def term(self):
        value = self.unary()
        while self.peek() in ("*", "/", "%"):
            op = self.take()
            right = self.unary()
            if op == "*":
                value *= right
            elif right == 0:
                raise ValueError("division by zero")
            elif op == "/":
                value /= right
            else:
                value %= right
        return value

    def unary(self):
        if self.peek() in ("+", "-"):
            sign = self.take()
            value = self.unary()
            return -value if sign == "-" else value
        return self.power()

    def power(self):
        value = self.atom()
        if self.peek() == "**":
            self.take()
            try:
                value = value ** self.unary()
            except ZeroDivisionError:
                raise ValueError("division by zero")
            if isinstance(value, complex):
                raise ValueError("not a real number")
        return value

    def atom(self):
        token = self.take()
        if token == "(":
            value = self.sum()
            if self.take() != ")":
                raise ValueError("missing )")
            return value
        if token is None or token in ("+", "-", "*", "/", "%", ")", "**"):
            raise ValueError("expected a number")
        return float(token)


def evaluate(text):
    parser = Parser(tokenize(text))
    value = parser.sum()
    if parser.peek() is not None:
        raise ValueError("trailing input")
    return float(value)
PY
