#!/bin/sh
# release.sh — one-command version bump + gates + tag + publish.
#
# Bakes in the release checklist so no step can be skipped (the P3B
# release missed the openapi pin and a ledger note — this script
# exists so that never happens again):
#   1. tree clean, on rust branch, all 6 version pins agree
#   2. bump pins, rebuild lockfile
#   3. full test suite + clippy gates (AFTER the bump)
#   4. commit + push, tag + push tag
#   5. portable release build -> tarball + SHA256SUMS -> gh release
#   6. optional: native service build + restart + verify-deploy
#
# Usage: ./release.sh X.Y.Z [--service] [--crosscheck] [--dry-run]
set -u

VER="${1:-}"
SERVICE=0; CROSSCHECK=0; DRY=0
for a in "$@"; do
    case "$a" in
        --service) SERVICE=1 ;;
        --crosscheck) CROSSCHECK=1 ;;
        --dry-run) DRY=1 ;;
    esac
done

case "$VER" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) echo "usage: $0 X.Y.Z [--service] [--crosscheck] [--dry-run]" >&2; exit 1 ;;
esac

ROOT=$(dirname "$0")
cd "$ROOT" || exit 1

run() { # dry-run aware
    if [ "$DRY" = 1 ]; then echo "dry-run: $*"; else "$@"; fi
}

# ---- 1. preconditions (read-only, always run) ----
BRANCH=$(git branch --show-current)
[ "$BRANCH" = "rust" ] || { echo "must run on rust branch (on $BRANCH)" >&2; exit 1; }
DIRTY=$(git status --porcelain | grep -v '^??' || true)
[ -z "$DIRTY" ] || { echo "tree must be clean; found:" >&2; echo "$DIRTY" >&2; exit 1; }
CUR=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
[ -n "$CUR" ] || { echo "cannot read current version" >&2; exit 1; }
echo "bump $CUR -> $VER"
for f in Cargo.toml Cargo.lock README.md docs/api.md assets/static/openapi.json; do
    grep -qF "$CUR" "$f" || { echo "pin desync: $f lacks $CUR" >&2; exit 1; }
done
echo "pins agree on $CUR"
[ "$DRY" = 1 ] && { echo "dry-run: would bump, gate, commit, tag v$VER, build, publish"; exit 0; }

# ---- 2. bump ----
sed -i "s/$CUR/$VER/g" Cargo.toml README.md docs/api.md assets/static/openapi.json
cargo build --quiet || exit 1   # refreshes Cargo.lock
grep -qF "$VER" Cargo.lock || { echo "lockfile did not pick up $VER" >&2; exit 1; }

# ---- 3. gates AFTER the bump ----
cargo test --quiet || { echo "TESTS FAILED at $VER — tree left dirty for inspection" >&2; exit 1; }
[ "$(cargo clippy --all-targets 2>&1 | grep -cE '^(error|warning)')" = "0" ] \
    || { echo "CLIPPY FAILED at $VER" >&2; exit 1; }

# ---- 4. commit + tag ----
git add Cargo.toml Cargo.lock README.md docs/api.md assets/static/openapi.json
git commit -m "Release v$VER" || exit 1
git push origin rust || exit 1
git tag "v$VER" || exit 1
git push origin "v$VER" || exit 1

# ---- 5. portable build + publish ----
cargo build --release --quiet || exit 1
PKGDIR=/tmp/rel-$VER
rm -rf "$PKGDIR" && mkdir -p "$PKGDIR/pkg"
cp target/release/glances-rs "$PKGDIR/pkg/glances-rs"
tar -czf "$PKGDIR/glances-rs-linux-x86_64.tar.gz" -C "$PKGDIR/pkg" glances-rs
(cd "$PKGDIR" && sha256sum glances-rs-linux-x86_64.tar.gz > SHA256SUMS)
gh release create "v$VER" --target rust --title "glances-rs v$VER" \
    --notes "Release v$VER. Pure-std, zero crates, LGPL-3.0-only." \
    "$PKGDIR/glances-rs-linux-x86_64.tar.gz" "$PKGDIR/SHA256SUMS" || exit 1
echo "published https://github.com/UberMetroid/glances-rs/releases/tag/v$VER"

# ---- 6. service cutover (optional) ----
if [ "$SERVICE" = 1 ]; then
    RUSTFLAGS='-C target-cpu=native' cargo build --release --quiet || exit 1
    # Stop BEFORE copy: the running service holds the binary (ETXTBSY).
    systemctl --user stop glances-rs
    sleep 2
    cp target/release/glances-rs "$HOME/.local/bin/glances-rs"
    systemctl --user start glances-rs
    sleep 5
    ./verify-deploy.sh || exit 1
fi
if [ "$CROSSCHECK" = 1 ]; then
    ./crosscheck.sh http://localhost:61208 || exit 1
fi
echo "release v$VER complete"
