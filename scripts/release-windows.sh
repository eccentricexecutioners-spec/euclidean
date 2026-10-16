#!/usr/bin/env bash
# Release Euclidean for Windows x64 through Cargo Bay, from the Mac Mini. See docs/releasing.md § Euclidean for Windows.
#
#   scripts/release-windows.sh <version> [--run <id> | --zip <path>] [--dry-run] [--notes "What's new"]
#
# GitHub builds the exe (.github/workflows/euclidean-windows.yml); this script releases the commit the fork's tag
# euclidean-v<version> already names (scripts/release-mac.sh makes that tag). It makes no tag and changes no tag.
#
# 1. Refuses unless tag euclidean-v<version> exists here, and finds the newest successful euclidean-windows.yml run whose
#    head SHA is that tag's commit (or takes --run <id>, refused when its head SHA is not that commit). Downloads the
#    run's artifact with `gh run download` into <release dir>/<version>-windows/download/. With --zip <path> (when gh is
#    not available here) it takes that zip and the build-info.txt beside it instead; Gate 3 still applies.
# 2. Unzips the artifact's one Euclidean/ folder and commits it to a one-off git repo, tagged v<version>, as
#    release-mac.sh does with Euclidean.app: Cargo Bay's release tool zips with `git archive` from a tag.
# 3. Has the release tool build and sign that zip (--platform windows, the Mini's release key; its --dry-run), then
#    gates it, every gate before anything is sent:
#      GATE 1  the zip's one top level is Euclidean/, holding Euclidean.exe; no __MACOSX
#      GATE 2  Euclidean.exe is a PE32+ x86-64 GUI executable (`file`)
#      GATE 3  build-info.txt names the tag's commit and <version> (and the run's head SHA is that commit); the exe in
#              the zip is byte for byte the one GitHub built
#      GATE 4  D3's uplink rules for a windows release, run by crates/uplink itself (signature, size, sha256, entries,
#              then its own unzip and tree comparison)
#      GATE 5  the zip is under 524,288,000 bytes (500 MB)
# 4. Unless --dry-run: runs the release tool for real (POST /releases, upload, finalize).
#
# Environment:
#   CARGO_BAY_DIR             the cargo-bay checkout (default ~/Eccentric-Executioners-code/cargo-bay); the tool runs there
#   CARGO_BAY_RELEASE_KEY     the release key (default: the one release-signing-rk-*.key in ~/.cargo-bay/release-keys/)
#   CARGO_BAY_PUBLISH_KEY     the publish key (default: read from ~/.cargo-bay/release-keys/publish-key)
#   EUCLIDEAN_RELEASE_DIR     where each release's folder goes (default ~/euclidean-releases); never overwritten
#   EUCLIDEAN_GITHUB_REPO     the fork on GitHub (default eccentricexecutioners-spec/euclidean)
set -euo pipefail

LIMIT=524288000
TOOL=euclidean
WORKFLOW=euclidean-windows.yml
ARTIFACT=euclidean-windows-x64
MOTHERSHIP_PAGE="https://eccentricexecutioners.ca/dashboard/cargo-bay/${TOOL}"

die() { printf 'release-windows: %s\n' "$*" >&2; exit 1; }
gate_fail() { printf 'release-windows: GATE %s: FAIL — %s\n' "$1" "$2" >&2; return 1; }
gate_pass() { printf 'release-windows: GATE %s: PASS — %s\n' "$1" "$2"; }

# GATE 1: <zip>. One top level, Euclidean/; Euclidean/Euclidean.exe a file in it; no __MACOSX anywhere.
gate1_zip() {
  local zip="$1" names tops
  names="$(zipinfo -1 "$zip" 2>/dev/null)" || { gate_fail 1 "not a zip: $zip"; return 1; }
  tops="$(printf '%s\n' "$names" | cut -d/ -f1 | sort -u)"
  [ "$tops" = "Euclidean" ] || { gate_fail 1 "the zip's top level is not exactly Euclidean/: $(echo $tops)"; return 1; }
  printf '%s\n' "$names" | grep -q -E '(^|/)__MACOSX(/|$)' && { gate_fail 1 "the zip holds __MACOSX"; return 1; }
  printf '%s\n' "$names" | grep -q -x 'Euclidean/Euclidean.exe' || { gate_fail 1 "no Euclidean/Euclidean.exe in the zip"; return 1; }
  gate_pass 1 "one top-level Euclidean/ holding Euclidean.exe, no __MACOSX ($(printf '%s\n' "$names" | wc -l | tr -d ' ') entries: $(echo $names))"
}

# GATE 2: <exe>. `file` must call it a PE32+ GUI executable for x86-64.
gate2_exe() {
  local exe="$1" kind
  [ -f "$exe" ] || { gate_fail 2 "no file at $exe"; return 1; }
  kind="$(file -b "$exe")"
  case "$kind" in
    *"PE32+ executable"*) ;;
    *) gate_fail 2 "not a PE32+ executable: $kind"; return 1 ;;
  esac
  case "$kind" in *"(GUI)"*) ;; *) gate_fail 2 "not a GUI-subsystem executable: $kind"; return 1 ;; esac
  case "$kind" in *"x86-64"*) ;; *) gate_fail 2 "not x86-64: $kind"; return 1 ;; esac
  gate_pass 2 "Euclidean.exe: $kind"
}

# GATE 3: <build-info.txt> <commit> <version> [<run head SHA>]. The build is of the tagged commit and this version.
gate3_build_info() {
  local info="$1" want_sha="$2" want_version="$3" run_sha="${4:-}" sha version
  [ -f "$info" ] || { gate_fail 3 "no build-info.txt at $info"; return 1; }
  sha="$(sed -n 's/^sha=//p' "$info" | head -1)"
  version="$(sed -n 's/^version=//p' "$info" | head -1)"
  [ "$sha" = "$want_sha" ] || { gate_fail 3 "build-info sha ${sha:-none} is not the tag's commit $want_sha"; return 1; }
  [ "$version" = "$want_version" ] || { gate_fail 3 "build-info version ${version:-none} is not $want_version"; return 1; }
  if [ -n "$run_sha" ] && [ "$run_sha" != "$want_sha" ]; then
    gate_fail 3 "the run's head SHA $run_sha is not the tag's commit $want_sha"; return 1
  fi
  local run_line=""
  [ -z "$run_sha" ] || run_line="; run head SHA $run_sha = the tag's commit"
  gate_pass 3 "build-info sha $sha = the tag's commit; version $version$run_line"
}

# Sourced (by a test of the gates): stop here, with the gate functions defined.
if [ "${BASH_SOURCE[0]}" != "$0" ]; then return 0; fi

VERSION="" DRY=0 NOTES="" RUN_ID="" ZIP_IN=""
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) DRY=1 ;;
    --notes) [ $# -ge 2 ] || die "--notes needs a value"; NOTES="$2"; shift ;;
    --run) [ $# -ge 2 ] || die "--run needs a run id"; RUN_ID="$2"; shift ;;
    --zip) [ $# -ge 2 ] || die "--zip needs a path"; ZIP_IN="$2"; shift ;;
    -*) die "unknown argument: $1" ;;
    *) [ -z "$VERSION" ] || die "one version only"; VERSION="$1" ;;
  esac
  shift
done
[ -n "$VERSION" ] || die "usage: scripts/release-windows.sh <version> [--run <id> | --zip <path>] [--dry-run] [--notes \"What's new\"]"
[[ "$VERSION" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-beta\.(0|[1-9][0-9]*))?$ ]] \
  || die "version must look like 1.2.3 or 1.2.3-beta.4 (Cargo Bay's rule); got $VERSION"
[ -z "$RUN_ID" ] || [ -z "$ZIP_IN" ] || die "give --run or --zip, not both"
[ -z "$RUN_ID" ] || [[ "$RUN_ID" =~ ^[0-9]+$ ]] || die "--run takes a numeric run id; got $RUN_ID"
NOTES="${NOTES:-Euclidean $VERSION}"
TAG="v$VERSION"                 # the release tool reads version and channel from this tag (staging repo)
FORK_TAG="euclidean-v$VERSION"  # this fork's tag, made by release-mac.sh

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
CARGO_BAY_DIR="${CARGO_BAY_DIR:-$HOME/Eccentric-Executioners-code/cargo-bay}"
RELEASE_TOOL="$CARGO_BAY_DIR/tools/release.mjs"
OUT_BASE="${EUCLIDEAN_RELEASE_DIR:-$HOME/euclidean-releases}"
OUT="$OUT_BASE/$VERSION-windows"
REPO="${EUCLIDEAN_GITHUB_REPO:-eccentricexecutioners-spec/euclidean}"

# --- 1. Refusals ---------------------------------------------------------------------------------------------------
[ "$(uname -s)" = Darwin ] || die "macOS only (the Mini)"
TAG_SHA="$(git rev-parse -q --verify "refs/tags/$FORK_TAG^{commit}")" \
  || die "REFUSED: no tag $FORK_TAG here; release Mac $VERSION first (scripts/release-mac.sh tags the commit GitHub built)"
[ -f "$RELEASE_TOOL" ] || die "no Cargo Bay release tool at $RELEASE_TOOL (set CARGO_BAY_DIR)"
command -v node >/dev/null || die "node is not installed"
if [ -z "$ZIP_IN" ]; then command -v gh >/dev/null || die "gh is not installed (or pass --zip)"; fi
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
echo "release-windows: Euclidean $VERSION for Windows x64; $FORK_TAG is commit $TAG_SHA"

# --- 2. The GitHub build of that commit -------------------------------------------------------------------------------
DOWNLOAD="$OUT/download"
mkdir -p "$DOWNLOAD"
RUN_SHA=""
if [ -n "$ZIP_IN" ]; then
  [ -f "$ZIP_IN" ] || die "no zip at $ZIP_IN"
  [ -f "$(dirname "$ZIP_IN")/build-info.txt" ] || die "no build-info.txt beside $ZIP_IN (Gate 3 needs it)"
  cp "$ZIP_IN" "$(dirname "$ZIP_IN")/build-info.txt" "$DOWNLOAD/"
  echo "release-windows: taking $ZIP_IN and its build-info.txt (no run SHA check: --zip)"
else
  if [ -z "$RUN_ID" ]; then
    RUN_ID="$(gh run list --repo "$REPO" --workflow "$WORKFLOW" --limit 100 --json databaseId,headSha,status,conclusion \
      --jq "[.[] | select(.headSha == \"$TAG_SHA\" and .status == \"completed\" and .conclusion == \"success\")][0].databaseId // empty")"
    [ -n "$RUN_ID" ] || die "REFUSED: no successful $WORKFLOW run of $TAG_SHA in $REPO (push the fork and run the workflow)"
  fi
  run="$(gh run view "$RUN_ID" --repo "$REPO" --json headSha,status,conclusion,workflowName \
    --jq '[.headSha, .status, .conclusion, .workflowName] | join(" ")')"
  read -r RUN_SHA run_status run_conclusion run_workflow <<<"$run"
  echo "release-windows: run $RUN_ID: $run_workflow, $run_status/$run_conclusion, head $RUN_SHA"
  [ "$RUN_SHA" = "$TAG_SHA" ] || die "REFUSED: run $RUN_ID built $RUN_SHA, not $FORK_TAG ($TAG_SHA)"
  [ "$run_status" = completed ] && [ "$run_conclusion" = success ] || die "REFUSED: run $RUN_ID is $run_status/$run_conclusion"
  gh run download "$RUN_ID" --repo "$REPO" --name "$ARTIFACT" --dir "$DOWNLOAD"
fi
ART_ZIP="$DOWNLOAD/Euclidean-$VERSION-windows-x64.zip"
[ -f "$ART_ZIP" ] || die "the artifact has no Euclidean-$VERSION-windows-x64.zip: $(ls "$DOWNLOAD")"
echo "release-windows: artifact zip $(stat -f %z "$ART_ZIP") bytes, sha256 $(shasum -a 256 < "$ART_ZIP" | cut -d' ' -f1)"
sed 's/^/release-windows: build-info: /' "$DOWNLOAD/build-info.txt"

# --- 3. Stage the Euclidean/ folder as a git tag (Cargo Bay's release tool zips with git archive) ----------------------
ART_UNZIP="$OUT/unzipped-artifact"
mkdir -p "$ART_UNZIP"
unzip -q "$ART_ZIP" -d "$ART_UNZIP"
[ "$(ls -A "$ART_UNZIP")" = "Euclidean" ] || die "the artifact zip's top level is not exactly Euclidean/: $(ls -A "$ART_UNZIP" | tr '\n' ' ')"
[ -f "$ART_UNZIP/Euclidean/Euclidean.exe" ] || die "the artifact zip has no Euclidean/Euclidean.exe"
STAGE="$OUT/stage"
mkdir -p "$STAGE"
cp -R "$ART_UNZIP/Euclidean" "$STAGE/Euclidean"
git -C "$STAGE" init -q
git -C "$STAGE" -c core.autocrlf=false add Euclidean
git -C "$STAGE" -c user.name="Euclidean release" -c user.email="release@localhost" commit -q \
  -m "Euclidean $VERSION for Windows x64 ($TAG_SHA${RUN_ID:+, run $RUN_ID})"
git -C "$STAGE" tag "$TAG"

# --- 4. Build, sign and check the zip with the release tool's dry run -------------------------------------------------
release_tool() { (cd "$CARGO_BAY_DIR" && node tools/release.mjs --repo "$STAGE" --tag "$TAG" --path Euclidean \
  --tool "$TOOL" --kind desktop-app --platform windows --notes "$NOTES" --key "$CARGO_BAY_RELEASE_KEY" "$@"); }
release_tool --dry-run "$OUT/signed"
ZIP="$OUT/signed/Euclidean-$VERSION.zip"
[ -f "$ZIP" ] || die "the release tool left no $ZIP"
BYTES="$(stat -f %z "$ZIP")"
echo "release-windows: zip $BYTES bytes; exe $(stat -f %z "$STAGE/Euclidean/Euclidean.exe") bytes; key $KEY_ID"
python3 -I -c 'import json,sys; m=json.load(open(sys.argv[1])); sys.exit(0 if m.get("platform")=="windows" else 1)' \
  "$OUT/signed/manifest.json" || die "the signed manifest does not carry platform windows"
echo "release-windows: the signed manifest carries platform windows"

gate1_zip "$ZIP" || die "GATE 1 FAIL; nothing sent"
TOOL_UNZIP="$OUT/unzipped-tool"
mkdir -p "$TOOL_UNZIP"
unzip -q "$ZIP" -d "$TOOL_UNZIP"
gate2_exe "$TOOL_UNZIP/Euclidean/Euclidean.exe" || die "GATE 2 FAIL; nothing sent"
gate3_build_info "$DOWNLOAD/build-info.txt" "$TAG_SHA" "$VERSION" "$RUN_SHA" || die "GATE 3 FAIL; nothing sent"
cmp -s "$TOOL_UNZIP/Euclidean/Euclidean.exe" "$ART_UNZIP/Euclidean/Euclidean.exe" \
  || die "GATE 3 FAIL: the exe in the release zip is not the one GitHub built; nothing sent"
echo "release-windows: GATE 3: the exe in the release zip is byte for byte the artifact's (sha256 $(shasum -a 256 < "$TOOL_UNZIP/Euclidean/Euclidean.exe" | cut -d' ' -f1))"
# 4. D3's uplink rules, run by the uplink crate itself, for a windows release: signature (the release key as a listed
#    key), the signed platform, size, sha256, entry rules, then its own unzip and tree comparison.
CHECKER="$OUT/zipcheck"
mkdir -p "$CHECKER/src"
cat > "$CHECKER/Cargo.toml" <<TOML
[package]
name = "euclidean-zipcheck"
version = "0.0.0"
edition = "2021"
publish = false

[workspace]

[dependencies]
vectorcraft-uplink = { path = "$ROOT/crates/uplink" }
serde_json = "1"
TOML
cat > "$CHECKER/src/main.rs" <<'RUST'
use std::path::Path;
use vectorcraft_uplink::trust::{parse_spki_pem, Trust};
use vectorcraft_uplink::zipcheck::{self, Layout};

fn read(p: &str) -> Result<String, String> {
    std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))
}

fn run() -> Result<String, String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, zip, manifest, sig, public, dest] = args.as_slice() else {
        return Err("usage: euclidean-zipcheck <zip> <manifest.json> <manifest.json.sig> <release .pub> <new dir>".into());
    };
    let key = parse_spki_pem(&read(public)?)?;
    let trust = Trust { master: [0u8; 32], extra_masters: Vec::new(), release: vec![key] };
    let manifest: serde_json::Value = serde_json::from_str(&read(manifest)?).map_err(|e| e.to_string())?;
    let entries = zipcheck::verify_package_as(&trust, Path::new(zip), &manifest, read(sig)?.trim(), Layout::Windows)?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    zipcheck::extract(Path::new(zip), &entries, Path::new(dest))?;
    zipcheck::tree_matches_in(&entries, &Path::new(dest).join(Layout::Windows.root()), Layout::Windows)?;
    Ok(format!("signature, platform windows, size, sha256 and entry rules pass ({} entries); unzipped tree matches the zip", entries.len()))
}

fn main() {
    match run() {
        Ok(line) => println!("{line}"),
        Err(e) => {
            eprintln!("euclidean-zipcheck: {e}");
            std::process::exit(1);
        }
    }
}
RUST
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
line="$(CARGO_TARGET_DIR="$TARGET/zipcheck" cargo run -q --release --manifest-path "$CHECKER/Cargo.toml" -- \
  "$ZIP" "$OUT/signed/manifest.json" "$OUT/signed/manifest.json.sig" "${CARGO_BAY_RELEASE_KEY%.key}.pub" "$OUT/unzipped-uplink")" \
  || die "GATE 4 FAIL: the zip fails D3's uplink rules for windows; nothing sent"
gate_pass 4 "D3 uplink: $line"
# 5. Under the 500 MB limit.
[ "$BYTES" -lt "$LIMIT" ] || die "GATE 5 FAIL: the zip is $BYTES bytes, at or over $LIMIT; nothing sent"
gate_pass 5 "$BYTES bytes < $LIMIT"

# The real run zips the same tag again; git archive stamps entries with the current time, so that zip differs from the
# checked one in its time stamps only. Checked here on a second zip and after the release on the one sent (as Mac).
same_but_times() {
  python3 -I - "$1" "$2" <<'PY'
import os, struct, sys, zipfile
a, b = sys.argv[1:]
def untimed(extra):
    # Zero the payload of the extended-timestamp field (0x5455, "UT"); keep every other byte.
    out, i = b"", 0
    while i + 4 <= len(extra):
        hid, n = struct.unpack("<HH", extra[i:i + 4])
        body = extra[i + 4:i + 4 + n]
        out += extra[i:i + 4] + (bytes(len(body)) if hid == 0x5455 else body)
        i += 4 + n
    return out + extra[i:]
key = lambda z: [(e.filename, e.CRC, e.compress_size, e.file_size, e.external_attr, e.compress_type, e.create_system,
                  e.flag_bits, e.header_offset, untimed(e.extra)) for e in z.infolist()]
ok = os.path.getsize(a) == os.path.getsize(b) and key(zipfile.ZipFile(a)) == key(zipfile.ZipFile(b))
sys.exit(0 if ok else 1)
PY
}
release_tool --dry-run "$OUT/signed-again" >/dev/null
same_but_times "$ZIP" "$OUT/signed-again/Euclidean-$VERSION.zip" \
  || die "a second zip of this tag differs from the checked one beyond time stamps; nothing sent"
echo "release-windows: a second zip of this tag matches the checked one in everything but time stamps"

if [ "$DRY" = 1 ]; then
  echo "release-windows: dry run: $ZIP ($BYTES bytes), manifest signed by $KEY_ID, platform windows. Nothing sent."
  exit 0
fi

# --- 5. Release ---------------------------------------------------------------------------------------------------------
CARGO_BAY_PUBLISH_KEY="$CARGO_BAY_PUBLISH_KEY" release_tool | tee "$OUT/release.txt"
sent="$(ls -t "${TMPDIR:-/tmp}"/cargo-bay-release-*/"Euclidean-$VERSION.zip" 2>/dev/null | head -1 || true)"
if [ -n "$sent" ] && same_but_times "$ZIP" "$sent"; then
  echo "release-windows: the zip sent ($sent, sha256 $(shasum -a 256 < "$sent" | cut -d' ' -f1)) matches the checked one but for time stamps" | tee -a "$OUT/release.txt"
else
  echo "release-windows: WARNING: could not match the zip sent (${sent:-not found}) to the checked one; check it by hand" | tee -a "$OUT/release.txt"
fi
echo "release-windows: released $FORK_TAG ($TAG_SHA${RUN_ID:+, GitHub run $RUN_ID}) for Windows x64, signed by $KEY_ID. No tag made."
echo "release-windows: Mothership tool page: $MOTHERSHIP_PAGE"
