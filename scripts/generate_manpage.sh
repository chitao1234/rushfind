#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
scdoc < docs/rfd.1.scd > docs/rfd.1
