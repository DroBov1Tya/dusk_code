#!/bin/sh
# The projects of three of the short tasks, each in its own folder.
set -e
tasks="$(dirname "$0")/.."
cp -R "$tasks/add-cli-flag/fixture" report
cp -R "$tasks/fix-pagination/fixture" pager
cp -R "$tasks/thread-a-setting/fixture" settings
