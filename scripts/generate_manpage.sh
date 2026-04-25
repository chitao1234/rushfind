#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
scdoc < doc/rfd.1.scd > doc/rfd.1
