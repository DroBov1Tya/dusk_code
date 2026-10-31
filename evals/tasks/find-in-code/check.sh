#!/bin/sh
set -e
git diff --quiet HEAD
test -z "$(git status --porcelain)"
grep -q "DUSK_UPLOAD_TRIES" "$EVAL_ANSWER"
grep -Eqi "(\b4\b|four|четыр)" "$EVAL_ANSWER"
