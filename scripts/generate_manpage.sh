#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

# scdoc stamps the page with the current date unless SOURCE_DATE_EPOCH is set.
# Pin it to the author date of the last commit that touched the source, so a
# regeneration reproduces the checked-in page instead of carrying a date-only
# diff. Author dates survive rebase and cherry-pick. Outside a git tree the
# stamp falls back to today; the freshness check ignores the stamp either way.
epoch=$(git log -1 --format=%at -- docs/rfd.1.scd 2>/dev/null || true)
if [ -n "$epoch" ]; then
    SOURCE_DATE_EPOCH=$epoch
    export SOURCE_DATE_EPOCH
fi

scdoc < docs/rfd.1.scd > docs/rfd.1
