#!/bin/sh
# verify-deploy.sh — end-to-end gate check against a RUNNING deployment.
#
# Usage:
#   GLANCES_API_KEY=<key> ./verify-deploy.sh [base-url] [expected-version]
#   ./verify-deploy.sh                        # ungated deployment
#
# With a key: expects 401s without it and 200s with it. Without a key
# (blank/unset): expects everything open. Prints one line per check,
# exits nonzero on the first failure.
set -e

BASE="${1:-http://localhost:61208}"
KEY="${GLANCES_API_KEY:-}"
if [ -n "${2:-}" ]; then
    EXPECTED_VERSION="$2"
else
    ROOT=$(dirname "$0")
    EXPECTED_VERSION=$(grep -m1 '^version' "$ROOT/Cargo.toml" | sed -E 's/.*"([^"]+)".*/\1/')
fi
[ -n "$EXPECTED_VERSION" ] || { echo "cannot determine expected version" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || { echo "curl is required" >&2; exit 1; }

PASS=0
check() { # name expected actual
    if [ "$2" = "$3" ]; then
        PASS=$((PASS + 1))
        echo "ok $PASS - $1"
    else
        echo "FAIL - $1 (expected $2, got $3)" >&2
        exit 1
    fi
}
code() { # path [key]
    if [ -n "${2:-}" ]; then
        curl -s -o /dev/null -w '%{http_code}' -H "X-API-Key: $2" "$BASE$1"
    else
        curl -s -o /dev/null -w '%{http_code}' "$BASE$1"
    fi
}
get() { # path; uses $AUTH when set
    if [ -n "$AUTH" ]; then
        curl -s -H "X-API-Key: $AUTH" "$BASE$1"
    else
        curl -s "$BASE$1"
    fi
}

if [ -n "$KEY" ]; then
    MODE="gated"
    AUTH="$KEY"
    check "dashboard shell open" "200" "$(code /)"
    check "api without key -> 401" "401" "$(code /api/4/cpu)"
    if curl -s -o /dev/null -D - "$BASE/api/4/cpu" | grep -qi 'www-authenticate'; then
        echo "FAIL - key-only 401 must not challenge Basic" >&2; exit 1
    fi
    PASS=$((PASS + 1)); echo "ok $PASS - no Basic challenge on 401"
    check "api wrong key -> 401" "401" "$(code /api/4/cpu wrong)"
    check "api right key -> 200" "200" "$(code /api/4/cpu "$KEY")"
    check "openapi gated -> 401" "401" "$(code /openapi.json)"
    check "openapi with key -> 200" "200" "$(code /openapi.json "$KEY")"
else
    MODE="open"
    AUTH=""
    check "dashboard shell open" "200" "$(code /)"
    check "api open -> 200" "200" "$(code /api/4/cpu)"
fi
check "version is $EXPECTED_VERSION" "$EXPECTED_VERSION" \
    "$(get /api/4/status | grep -o '"version":"[^"]*"' | head -n1 | sed -E 's/.*"([^"]+)".*/\1/')"
if [ -n "$(get /api/4/processlist | grep -o '"pid"' | head -n1)" ]; then
    PASS=$((PASS + 1)); echo "ok $PASS - processlist non-empty"
else
    echo "FAIL - processlist empty" >&2; exit 1
fi
check "health 200" "200" "$(code /api/health "$AUTH")"
if [ "$(curl -s "$BASE/" | grep -c 'X-API-Key\|glances_key')" -ge 2 ]; then
    PASS=$((PASS + 1)); echo "ok $PASS - dashboard key markers present"
else
    echo "FAIL - dashboard key markers missing" >&2; exit 1
fi
echo "ALL CHECKS PASSED ($MODE mode, $PASS checks)"
