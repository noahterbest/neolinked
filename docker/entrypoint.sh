#!/bin/sh

# Allows Ctrl-C, by letting this sh process act as PID 1
exit_func() {
    exit 1
}
trap exit_func TERM INT

# Best effort only: raising the *hard* limit needs privileges the container
# usually does not have, and neolink raises its own soft limit to the hard
# limit at startup regardless. Don't fail or warn if this is not permitted.
ulimit -n 65535 2>/dev/null || true

echo "Running: ${*}"
"$@"
