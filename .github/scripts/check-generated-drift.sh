#!/usr/bin/env bash
#
# Fail if an example's committed generator output no longer matches what the
# generators emit.
#
# Call this *after* building the example: its build.rs regenerates in place, so
# a dirty working tree afterwards means the committed files are stale. Pass the
# example's directory, e.g. `examples/notes-kb`.
#
# Only paths containing "generated" are checked. Lockfiles and hand-written
# sources are deliberately out of scope — cargo may touch a lockfile for
# reasons that have nothing to do with codegen, and a spurious red build
# teaches people to ignore this check.
set -euo pipefail

if [ $# -ne 1 ]; then
    echo "usage: $0 <example-dir>" >&2
    exit 2
fi

example="${1%/}"
pathspec="${example}/*generated*"

if git diff --quiet -- "$pathspec"; then
    echo "✓ ${example}: committed generator output is current"
    exit 0
fi

# GitHub renders ::error:: as an annotation on the PR, so the reason shows up
# without opening the log.
echo "::error title=Stale generated output::${example} has committed generator output that no longer matches the generators. Run 'just regen-examples' and commit the result."

echo
echo "Files that would change:"
git diff --stat -- "$pathspec"
echo
echo "Diff:"
git --no-pager diff -- "$pathspec"

exit 1
