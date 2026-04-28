#!/bin/sh
# crypttab keyscript wrapper. cryptsetup runs this with the third
# crypttab field as $1 (typically "none") and the following env vars
# set:
#   CRYPTTAB_NAME, CRYPTTAB_SOURCE, CRYPTTAB_KEY, CRYPTTAB_TRIED
#
# We forward to the binary, which prints the derived passphrase to
# stdout (no trailing newline). If the binary fails, we print nothing
# and exit non-zero — cryptsetup will then prompt on tty as a
# fallback.
exec /sbin/luks-controller-unlock keyscript --name "${CRYPTTAB_NAME:-root}"
