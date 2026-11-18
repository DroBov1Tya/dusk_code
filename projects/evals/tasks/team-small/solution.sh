#!/bin/sh
set -e
tasks="$(dirname "$0")/.."
(cd report && sh "$tasks/add-cli-flag/solution.sh")
(cd pager && sh "$tasks/fix-pagination/solution.sh")
(cd settings && sh "$tasks/thread-a-setting/solution.sh")
