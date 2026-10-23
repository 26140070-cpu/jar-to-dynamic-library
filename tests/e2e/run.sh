#!/bin/sh
set -eu
cd "$(dirname "$0")"
ROOT=$(cd ../.. && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/harness" "$WORK/mod" "$WORK/home"
javac --release 8 -d "$WORK/harness" $(find harness -name '*.java')
javac --release 8 -cp "$WORK/harness" -d "$WORK/mod" $(find mod -name '*.java')
(cd "$WORK/mod" && jar cf "$WORK/testmod.jar" testmod)

INPUT_JAR="$WORK/testmod.jar" cargo build --release --manifest-path "$ROOT/Cargo.toml"

case $(uname) in
  Darwin) LIB=libjar_to_dynamic_library.dylib ;;
  *)      LIB=libjar_to_dynamic_library.so ;;
esac

TIMEOUT=""
for candidate in timeout gtimeout; do
  if command -v "$candidate" >/dev/null 2>&1; then TIMEOUT="$candidate 60"; break; fi
done

$TIMEOUT "${E2E_JAVA:-java}" -Duser.home="$WORK/home" ${E2E_JAVA_OPTS:-} \
  -cp "$WORK/harness" Main "$ROOT/target/release/$LIB" "$WORK/mod" || {
  status=$?
  echo "--- injector log (test FAILED, exit $status) ---"
  cat "$WORK/home/jar-to-dynamic-library-log.txt" 2>/dev/null || echo "(no log written)"
  exit "$status"
}
echo "--- injector log ---"
cat "$WORK/home/jar-to-dynamic-library-log.txt"
echo "E2E OK"
