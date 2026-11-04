#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/jtdl-adapter-e2e.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
ulimit -c 0 2>/dev/null || true

command -v javac >/dev/null || { echo "javac is required" >&2; exit 2; }
command -v java >/dev/null || { echo "java is required" >&2; exit 2; }
command -v python3 >/dev/null || { echo "python3 is required" >&2; exit 2; }

mkdir -p "$WORK/agent-classes"
javac --release 8 -encoding UTF-8 -d "$WORK/agent-classes" "$ROOT/java/ModLoaderAgent.java"

python3 - "$WORK" <<'PY'
import hashlib
import pathlib
import sys
import zipfile

root = pathlib.Path(sys.argv[1])
for loader, metadata in (
    ("forge", "META-INF/mods.toml"),
    ("neoforge", "META-INF/neoforge.mods.toml"),
):
    mod_path = root / f"{loader}-sample.jar"
    mod_metadata = (
        'modLoader="javafml"\n'
        'loaderVersion="[1,)"\n'
        'license="MIT"\n'
        '[[mods]]\n'
        'modId="jtdl_agent_test"\n'
        'version="1.0.0"\n'
        'displayName="JTDL Agent Test"\n'
    )
    with zipfile.ZipFile(mod_path, "w", compression=zipfile.ZIP_DEFLATED) as jar:
        jar.writestr(metadata, mod_metadata)
        jar.writestr("assets/jtdl_agent_test/lang/en_us.json", '{"test.name":"Test"}')

    agent_path = root / f"{loader}-agent.jar"
    class_path = root / "agent-classes/io/jtdl/agent/ModLoaderAgent.class"
    manifest = (
        "Manifest-Version: 1.0\r\n"
        "Premain-Class: io.jtdl.agent.ModLoaderAgent\r\n"
        "Can-Redefine-Classes: false\r\n"
        "Can-Retransform-Classes: false\r\n"
        f"Jtdl-Loader: {loader}\r\n\r\n"
    )
    with zipfile.ZipFile(agent_path, "w", compression=zipfile.ZIP_DEFLATED) as jar:
        jar.writestr("META-INF/MANIFEST.MF", manifest)
        jar.write(class_path, "io/jtdl/agent/ModLoaderAgent.class")

    bundle_path = root / f"{loader}.jtdl"
    bundle_metadata = (
        "bundleVersion=1\n"
        f"loader={loader}\n"
        "embeddedPath=META-INF/jtdl/embedded-mod.jar\n"
        f"sourceFileName={loader}-sample\n"
        "modIds=jtdl_agent_test\n"
    )
    with zipfile.ZipFile(bundle_path, "w", compression=zipfile.ZIP_STORED) as bundle:
        bundle.writestr("META-INF/jtdl/bundle.properties", bundle_metadata)
        bundle.write(mod_path, "META-INF/jtdl/embedded-mod.jar")
    print(f"prepared synthetic {loader} input sha256={hashlib.sha256(mod_path.read_bytes()).hexdigest()}")
PY

for loader in forge neoforge; do
    mkdir -p "$WORK/$loader/mods"
    java "-Djtdl.modsDir=$WORK/$loader/mods" \
        "-javaagent:$WORK/$loader-agent.jar=$WORK/$loader.jtdl" -version \
        >"$WORK/$loader.log" 2>&1
    grep -F "The real $loader loader will now process the mod" "$WORK/$loader.log" >/dev/null
    extracted="$(find "$WORK/$loader/mods" -maxdepth 1 -type f -name '*.jar' -print -quit)"
    test -n "$extracted"
    python3 - "$WORK/$loader-sample.jar" "$extracted" <<'PY'
import hashlib
import pathlib
import sys
original, extracted = map(pathlib.Path, sys.argv[1:])
a = hashlib.sha256(original.read_bytes()).hexdigest()
b = hashlib.sha256(extracted.read_bytes()).hexdigest()
assert a == b, f"extracted JAR changed: {a} != {b}"
print(f"byte-for-byte preservation verified: {b}")
PY

    java "-Djtdl.modsDir=$WORK/$loader/mods" \
        "-javaagent:$WORK/$loader-agent.jar=$WORK/$loader.jtdl" -version \
        >"$WORK/$loader-repeat.log" 2>&1
    grep -F "Embedded mod was already extracted" "$WORK/$loader-repeat.log" >/dev/null
    echo "$loader adapter agent: PASS"
done


mkdir -p "$WORK/mismatch/mods"
if java "-Djtdl.modsDir=$WORK/mismatch/mods" \
    "-javaagent:$WORK/forge-agent.jar=$WORK/neoforge.jtdl" -version \
    >"$WORK/mismatch.log" 2>&1; then
    echo "ERROR: mismatched agent/bundle unexpectedly succeeded" >&2
    exit 1
fi
grep -F "this is the forge adapter, but the bundle targets neoforge" "$WORK/mismatch.log" >/dev/null

echo "loader mismatch rejection: PASS"


mkdir -p "$WORK/duplicate/mods"
cp "$WORK/forge-sample.jar" "$WORK/duplicate/mods/original-copy.jar"
if java "-Djtdl.modsDir=$WORK/duplicate/mods" \
    "-javaagent:$WORK/forge-agent.jar=$WORK/forge.jtdl" -version \
    >"$WORK/duplicate.log" 2>&1; then
    echo "ERROR: duplicate mod ID unexpectedly succeeded" >&2
    exit 1
fi
grep -F "already exists" "$WORK/duplicate.log" >/dev/null

echo "duplicate mod ID rejection: PASS"
