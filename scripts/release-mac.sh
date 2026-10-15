#!/usr/bin/env bash
# Release Euclidean for macOS through Cargo Bay, from the Mac Mini. See docs/releasing.md § Euclidean on Cargo Bay.
#
#   scripts/release-mac.sh <version> [--dry-run] [--notes "What's new"]
#
# 1. Refuses unless the fork has no tracked changes, Cargo.toml's version is <version>, and tag euclidean-v<version> is
#    new. (The fork keeps upstream VectorCraft's v0.1.0, v0.1.1… tags, so Euclidean's own tags carry a prefix.)
# 2. Builds the app only (profile release-slim: stripped, thin LTO), assembles Euclidean.app as `cargo xtask bundle`
#    lays it out (without vectorcraft-cli), and signs the whole bundle ad-hoc.
# 3. Commits the bundle to a one-off git repo in the release folder and tags it v<version>: Cargo Bay's release tool
#    zips with `git archive` from a tag, so the zip holds exactly one top-level Euclidean.app/ and nothing else.
# 4. Has Cargo Bay's release tool build, sign (the Mini's release key) and check that zip (its --dry-run), and stops
#    if the zip is 50 MB (52,428,800 bytes) or more, Supabase's upload limit.
# 5. Unless --dry-run: runs the release tool for real (POST /releases, upload, finalize), then tags euclidean-v<version>.
#
# Environment:
#   CARGO_BAY_DIR             the cargo-bay checkout (default ~/Eccentric-Executioners-code/cargo-bay); the tool runs there
#   CARGO_BAY_RELEASE_KEY     the release key (default: the one release-signing-rk-*.key in ~/.cargo-bay/release-keys/)
#   CARGO_BAY_PUBLISH_KEY     the publish key (default: read from ~/.cargo-bay/release-keys/publish-key)
#   EUCLIDEAN_RELEASE_DIR     where each release's folder goes (default ~/euclidean-releases); never overwritten
set -euo pipefail

LIMIT=52428800
TOOL=euclidean
MOTHERSHIP_PAGE="https://eccentricexecutioners.ca/dashboard/cargo-bay/${TOOL}"

die() { printf 'release-mac: %s\n' "$*" >&2; exit 1; }

VERSION="" DRY=0 NOTES=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --notes) [ $# -ge 2 ] || die "--notes needs a value"; NOTES="$2"; shift ;;
    -*) die "unknown argument: $1" ;;
    *) [ -z "$VERSION" ] || die "one version only"; VERSION="$1" ;;
  esac
  shift
done
[ -n "$VERSION" ] || die "usage: scripts/release-mac.sh <version> [--dry-run] [--notes \"What's new\"]"
[[ "$VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-beta\.(0|[1-9][0-9]*))?$ ]] \
  || die "version must look like 1.2.3 or 1.2.3-beta.4 (Cargo Bay's rule); got $VERSION"
NOTES="${NOTES:-Euclidean $VERSION}"
TAG="v$VERSION"                 # the release tool reads version and channel from this tag (staging repo)
FORK_TAG="euclidean-v$VERSION"  # this fork's tag

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
CARGO_BAY_DIR="${CARGO_BAY_DIR:-$HOME/Eccentric-Executioners-code/cargo-bay}"
RELEASE_TOOL="$CARGO_BAY_DIR/tools/release.mjs"
OUT_BASE="${EUCLIDEAN_RELEASE_DIR:-$HOME/euclidean-releases}"
OUT="$OUT_BASE/$VERSION"

# --- 1. Refusals ---------------------------------------------------------------------------------------------------
[ "$(uname -s)" = Darwin ] || die "macOS only"
[ -z "$(git status --porcelain --untracked-files=no)" ] || die "REFUSED: the fork has tracked changes; commit or stash them"
have="$(cargo xtask version 2>/dev/null | tail -1)"
[ "$have" = "$VERSION" ] || die "REFUSED: Cargo.toml says $have, not $VERSION (cargo xtask version set $VERSION, then commit)"
git rev-parse -q --verify "refs/tags/$FORK_TAG" >/dev/null && die "REFUSED: tag $FORK_TAG already exists here (versions are never reused)"
[ -f "$RELEASE_TOOL" ] || die "no Cargo Bay release tool at $RELEASE_TOOL (set CARGO_BAY_DIR)"
command -v node >/dev/null || die "node is not installed"
if [ -z "${CARGO_BAY_RELEASE_KEY:-}" ]; then
  keys=("$HOME"/.cargo-bay/release-keys/release-signing-rk-*.key)
  [ -f "${keys[0]}" ] || die "no release key in ~/.cargo-bay/release-keys/ (set CARGO_BAY_RELEASE_KEY)"
  [ "${#keys[@]}" -eq 1 ] || die "more than one release key in ~/.cargo-bay/release-keys/; set CARGO_BAY_RELEASE_KEY"
  CARGO_BAY_RELEASE_KEY="${keys[0]}"
fi
KEY_ID="$(basename "$CARGO_BAY_RELEASE_KEY" .key)"; KEY_ID="${KEY_ID#release-signing-}"
if [ "$DRY" = 0 ] && [ -z "${CARGO_BAY_PUBLISH_KEY:-}" ]; then
  pk="$HOME/.cargo-bay/release-keys/publish-key"
  [ -f "$pk" ] || die "CARGO_BAY_PUBLISH_KEY is not set and $pk does not exist"
  CARGO_BAY_PUBLISH_KEY="$(tr -d '[:space:]' < "$pk")"
fi
[ ! -e "$OUT" ] || die "REFUSED: $OUT already exists; nothing is overwritten"

# --- 2. Slim build, bundle, ad-hoc signature --------------------------------------------------------------------------
SHA="$(git rev-parse --short HEAD)"
echo "release-mac: building Euclidean $VERSION ($SHA), profile release-slim"
VECTORCRAFT_BUILD_SHA="$SHA" cargo build --profile release-slim -p vectorcraft
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
BIN="$TARGET/release-slim/vectorcraft"

STAGE="$OUT/stage"
APP="$STAGE/Euclidean.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/Euclidean"
cp assets/app-icon/vectorcraft.icns "$APP/Contents/Resources/Euclidean.icns"
SHORT="${VERSION%%-*}"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@SHORT_VERSION@/$SHORT/g" -e "s/@BUILD_SHA@/$SHA/g" \
  packaging/macos/Info.plist.in > "$APP/Contents/Info.plist"
grep -q '@' "$APP/Contents/Info.plist" && grep -n '@[A-Z_]*@' "$APP/Contents/Info.plist" && die "Info.plist placeholder left"
codesign --force --deep -s - "$APP"
codesign --verify --deep --strict "$APP"

# --- 3. The bundle as a git tag (Cargo Bay's release tool zips with git archive) --------------------------------------
git -C "$STAGE" init -q
git -C "$STAGE" add Euclidean.app
git -C "$STAGE" -c user.name="Euclidean release" -c user.email="release@localhost" commit -q -m "Euclidean $VERSION ($SHA)"
git -C "$STAGE" tag "$TAG"

# --- 4. Build, sign and check the zip with the release tool's dry run -------------------------------------------------
release_tool() { (cd "$CARGO_BAY_DIR" && node tools/release.mjs --repo "$STAGE" --tag "$TAG" --path Euclidean.app \
  --tool "$TOOL" --kind desktop-app --notes "$NOTES" --key "$CARGO_BAY_RELEASE_KEY" "$@"); }
release_tool --dry-run "$OUT/signed"
ZIP="$OUT/signed/Euclidean.app-$VERSION.zip"
BYTES="$(stat -f %z "$ZIP")"
echo "release-mac: zip $BYTES bytes (limit $LIMIT); binary $(stat -f %z "$APP/Contents/MacOS/Euclidean") bytes; key $KEY_ID"
[ "$BYTES" -lt "$LIMIT" ] || die "STOP: the zip is $BYTES bytes, at or over Cargo Bay's 50 MB limit; nothing sent"
tops="$(zipinfo -1 "$ZIP" | cut -d/ -f1 | sort -u)"
[ "$tops" = "Euclidean.app" ] || die "the zip's top level is not exactly Euclidean.app: $tops"
zipinfo -1 "$ZIP" | grep -q -E '(^|/)(__MACOSX|vectorcraft-cli)(/|$)' && die "the zip holds __MACOSX or vectorcraft-cli"
CHECK="$OUT/unzipped"
mkdir -p "$CHECK"
ditto -x -k "$ZIP" "$CHECK"
codesign --verify --deep --strict "$CHECK/Euclidean.app" || die "the unzipped bundle's signature does not verify"
echo "release-mac: unzipped bundle verifies (codesign --verify --deep --strict)"

if [ "$DRY" = 1 ]; then
  echo "release-mac: dry run: $ZIP ($BYTES bytes), manifest signed by $KEY_ID. Nothing sent, no tag made."
  exit 0
fi

# --- 5. Release, then tag ---------------------------------------------------------------------------------------------
CARGO_BAY_PUBLISH_KEY="$CARGO_BAY_PUBLISH_KEY" release_tool | tee "$OUT/release.txt"
git tag -a "$FORK_TAG" -m "Euclidean $VERSION, released through Cargo Bay (signed by $KEY_ID)"
echo "release-mac: tagged $FORK_TAG here (not pushed). Signed by $KEY_ID."
echo "release-mac: Mothership tool page: $MOTHERSHIP_PAGE"
