#!/bin/sh
set -e
tasks="$(dirname "$0")/.."
(cd report && sh "$tasks/add-cli-flag/check.sh")
(cd pager && sh "$tasks/fix-pagination/check.sh")
(cd settings && sh "$tasks/thread-a-setting/check.sh")
