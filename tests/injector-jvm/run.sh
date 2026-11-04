#!/usr/bin/env bash














set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TESTS="$ROOT/tests/injector-jvm"
WORK="$TESTS/work"
FABRIC_JAR="$WORK/examplemod.jar"
FORGE_JAR="$WORK/exampleforge.jar"
JAVA_USER_HOME="$(java -XshowSettings:properties -version 2>&1 \
    | sed -n 's/^[[:space:]]*user.home = //p' | head -n 1)"
[[ -n "$JAVA_USER_HOME" ]] || JAVA_USER_HOME="$HOME"
REPORT="$JAVA_USER_HOME/jar-to-dynamic-library-log.txt"
GAME_PID=""
GAME_DIR=""

fail() {
    echo "FAIL: $*" >&2
    if [[ -n "$GAME_DIR" && -f "$GAME_DIR.log" ]]; then
        echo "--- game log ($GAME_DIR.log) ---" >&2
        cat "$GAME_DIR.log" >&2
    fi
    if [[ -f "$REPORT" ]]; then
        echo "--- injector report ---" >&2
        cat "$REPORT" >&2
    fi
    exit 1
}

cleanup() {
    if [[ -n "$GAME_PID" ]] && kill -0 "$GAME_PID" 2>/dev/null; then
        stop_game
    fi
}
trap cleanup EXIT

command -v javac >/dev/null 2>&1 || fail "javac is required to run this test"
JAVA_MAJOR="$(java -version 2>&1 | awk -F'"' '/version/ { split($2, p, "."); if (p[1] == "1") print p[2]; else print p[1]; exit }')"
[[ -n "$JAVA_MAJOR" ]] || fail "could not determine the Java version"
ATTACH_FLAGS=()
if [[ "$JAVA_MAJOR" -ge 9 ]]; then
    ATTACH_FLAGS+=(--add-modules jdk.attach)
fi

echo "== preparing the workspace"
rm -rf "$WORK"
mkdir -p "$WORK/game-classes" "$WORK/transformed-classes" "$WORK/forge-classes" "$WORK/mod-classes" "$WORK/nested-classes" \
    "$WORK/forge-mod-classes" "$WORK/helper" \
    "$WORK/agent" "$WORK/native" "$WORK/forge" \
    "$WORK/stage/META-INF/jars" "$WORK/stage/assets/examplemod/lang" \
    "$WORK/forge-stage/META-INF"

echo "== compiling the fake Fabric game"
javac --release 8 -nowarn -d "$WORK/game-classes" \
    "$TESTS/game/Main.java" \
    "$TESTS/game/KnotClassLoader.java" \
    "$TESTS/game/net/fabricmc/api/ModInitializer.java" \
    "$TESTS/game/net/fabricmc/api/ClientModInitializer.java" \
    "$TESTS/game/net/fabricmc/api/DedicatedServerModInitializer.java" \
    "$TESTS/game/net/fabricmc/loader/api/entrypoint/PreLaunchEntrypoint.java" \
    "$TESTS/game/net/fabricmc/loader/api/FabricLoader.java" \
    "$TESTS/game/game/Target.java" \
    "$TESTS/game/org/spongepowered/asm/mixin/Mixin.java" \
    "$TESTS/game/org/spongepowered/asm/mixin/MixinEnvironment.java" \
    "$TESTS/game/org/spongepowered/asm/mixin/Mixins.java" \
    "$TESTS/game/org/spongepowered/asm/mixin/transformer/IMixinTransformer.java" \
    "$TESTS/game/org/spongepowered/asm/mixin/transformer/TestMixinProcessor.java" \
    "$TESTS/game/org/spongepowered/asm/mixin/transformer/TestMixinTransformer.java" \
    || fail "the fake Fabric game did not compile"
javac --release 8 -nowarn -d "$WORK/transformed-classes" "$TESTS/game-transformed/game/Target.java" \
    || fail "the transformed target fixture did not compile"
mkdir -p "$WORK/game-classes/jtdl/transformed/game"
cp "$WORK/transformed-classes/game/Target.class" "$WORK/game-classes/jtdl/transformed/game/Target.class"

echo "== compiling the fake Forge game"
javac --release 8 -nowarn -d "$WORK/forge-classes" \
    "$TESTS/forge-game/ForgeMain.java" \
    "$TESTS/forge-game/LaunchClassLoader.java" \
    "$TESTS/forge-game/net/minecraftforge/eventbus/api/IEventBus.java" \
    "$TESTS/forge-game/net/minecraftforge/fml/common/Mod.java" \
    "$TESTS/forge-game/net/minecraftforge/fml/ModContainer.java" \
    "$TESTS/forge-game/net/minecraftforge/fml/ModList.java" \
    "$TESTS/forge-game/net/minecraftforge/fml/javafmlmod/FMLJavaModLoadingContext.java" \
    || fail "the fake Forge game did not compile"

echo "== packaging the nested library"
javac --release 8 -nowarn -d "$WORK/nested-classes" "$TESTS/nested/nested/NestedHelper.java" \
    || fail "the nested library did not compile"
jar --create --file "$WORK/nested-lib.jar" -C "$WORK/nested-classes" . || fail "could not build nested-lib.jar"

echo "== compiling and packaging the Fabric mod"
javac --release 8 -nowarn -cp "$WORK/game-classes:$WORK/nested-lib.jar" -d "$WORK/mod-classes" \
    "$TESTS/mod/example/ExampleMod.java" \
    "$TESTS/mod/example/ExampleClient.java" \
    "$TESTS/mod/example/ExampleServer.java" \
    "$TESTS/mod/example/ExamplePreLaunch.java" \
    "$TESTS/mod/example/mixin/ExampleMixin.java" \
    "$TESTS/mod/example/MeteorAddon.java" \
    "$TESTS/mod/example/ExampleMeteorAddon.java" \
    || fail "the Fabric mod did not compile"
cp "$TESTS/mod/fabric.mod.json" "$WORK/stage/fabric.mod.json"
cp "$TESTS/mod/examplemod.mixins.json" "$WORK/stage/examplemod.mixins.json"
cp "$TESTS/mod/assets/examplemod/lang/es_es.json" "$WORK/stage/assets/examplemod/lang/es_es.json"
cp "$WORK/nested-lib.jar" "$WORK/stage/META-INF/jars/nested-lib.jar"
cp -r "$WORK/mod-classes/." "$WORK/stage/"
jar --create --file "$FABRIC_JAR" -C "$WORK/stage" . || fail "could not build examplemod.jar"

echo "== compiling and packaging the Forge mod"
javac --release 8 -nowarn -cp "$WORK/forge-classes" -d "$WORK/forge-mod-classes" \
    "$TESTS/forge-mod/example/ForgeMod.java" \
    "$TESTS/forge-mod/example/ForgeTwoArgMod.java" \
    || fail "the Forge mod did not compile"
cp -r "$WORK/forge-mod-classes/." "$WORK/forge-stage/"
cp "$TESTS/forge-mod/META-INF/mods.toml" "$WORK/forge-stage/META-INF/mods.toml"
jar --create --file "$FORGE_JAR" -C "$WORK/forge-stage" . || fail "could not build exampleforge.jar"

echo "== compiling AttachHelper"
HELPER_CLASSPATH="$WORK/helper"
JAVAC_ATTACH_ARGS=("${ATTACH_FLAGS[@]}")
if [[ "$JAVA_MAJOR" -lt 9 ]]; then
    TOOLS_JAR="$(dirname "$(readlink -f "$(command -v java)")")/../lib/tools.jar"
    [[ -f "$TOOLS_JAR" ]] || fail "Java 8 needs lib/tools.jar for the attach API"
    HELPER_CLASSPATH="$WORK/helper:$TOOLS_JAR"
    JAVAC_ATTACH_ARGS=(-cp "$TOOLS_JAR")
fi


javac "${JAVAC_ATTACH_ARGS[@]}" -nowarn -d "$WORK/helper" "$ROOT/java/AttachHelper.java" \
    || fail "AttachHelper did not compile"

echo "== building the native library and the Java agent"


INPUT_JAR="$FABRIC_JAR" cargo build --offline --release --lib --manifest-path "$ROOT/Cargo.toml" \
    || fail "cargo build --lib failed"
LIBRARY="$(ls "$ROOT"/target/release/libjar_to_dynamic_library.so \
                 "$ROOT"/target/release/libjar_to_dynamic_library.dylib 2>/dev/null | head -n 1)"
[[ -n "$LIBRARY" ]] || fail "the cdylib was not produced"
AGENT="$(ls -t "$ROOT"/target/release/build/jartodynamiclibrary-*/out/jtdl-agent.jar 2>/dev/null | head -n 1)"
[[ -n "$AGENT" ]] || fail "jtdl-agent.jar was not produced by build.rs"

start_game() {
    local label="$1" classpath="$2" main="$3"
    GAME_DIR="$WORK/$label"
    mkdir -p "$GAME_DIR"
    java "${ATTACH_FLAGS[@]}" -cp "$classpath" \
        -Djtdl.test.dir="$GAME_DIR" -Djtdl.test.env=CLIENT \
        "$main" "$GAME_DIR" > "$GAME_DIR.log" 2>&1 &
    GAME_PID=$!
    for _ in $(seq 1 150); do
        [[ -f "$GAME_DIR/game.pid" ]] && return 0
        kill -0 "$GAME_PID" 2>/dev/null || fail "the fake game exited before it was ready"
        sleep 0.1
    done
    fail "the fake game never wrote its pid file"
}

stop_game() {
    echo game-stop > "$GAME_DIR/game-stop"
    for _ in $(seq 1 150); do
        kill -0 "$GAME_PID" 2>/dev/null || break
        sleep 0.1
    done
    kill -0 "$GAME_PID" 2>/dev/null && kill "$GAME_PID" 2>/dev/null
    wait "$GAME_PID" 2>/dev/null
    GAME_PID=""
}

assert_file() {
    local file="$1" expected="$2"
    [[ -f "$file" ]] || fail "$(basename "$file") was never written: the code path did not run"
    grep -qF -- "$expected" "$file" \
        || fail "$(basename "$file") does not contain '$expected' (found: $(cat "$file"))"
}

assert_absent() {
    [[ -f "$1" ]] && fail "$(basename "$1") must not exist (found: $(cat "$1"))"
    return 0
}

assert_report() {
    grep -qF -- "$1" "$REPORT" || fail "the injector report does not contain '$1'"
}

assert_no_report() {
    grep -qF -- "$1" "$REPORT" && fail "the injector report must not contain '$1'"
    return 0
}

wait_for_report() {
    for _ in $(seq 1 300); do
        grep -qE "JTDL\|(ok|error)\|done" "$REPORT" 2>/dev/null && return 0
        kill -0 "$GAME_PID" 2>/dev/null || fail "the fake game died while waiting for the injection"
        sleep 0.1
    done
    fail "the injection produced no final report line in time"
}

run_route() {
    local label="$1" method="$2" artifact="$3" classpath="$4" main="$5" jar="$6"
    echo "== route: $label ($method)"
    rm -f "$REPORT"
    start_game "$label" "$classpath" "$main"
    local pid
    pid="$(cat "$GAME_DIR/game.pid")"
    java "${ATTACH_FLAGS[@]}" -cp "$HELPER_CLASSPATH" AttachHelper "$method" "$pid" "$artifact" \
        "jar=$jar;session=test-${label}" \
        > "$GAME_DIR/attach.log" 2>&1 \
        || fail "AttachHelper could not inject through $label: $(cat "$GAME_DIR/attach.log")"
    grep -q "OK" "$GAME_DIR/attach.log" || fail "AttachHelper did not confirm the $label injection"
    wait_for_report
    assert_report "JTDL|detail|session|test-${label}"
    if [[ "$label" == "agent" || "$label" == "native" ]]; then
        touch "$GAME_DIR/mixin-check"
        for _ in $(seq 1 100); do
            [[ -f "$GAME_DIR/mixin-after.txt" ]] && break
            sleep 0.05
        done
        assert_file "$GAME_DIR/mixin-before.txt" "original"
        if [[ "$label" == "agent" ]]; then
            assert_file "$GAME_DIR/mixin-after.txt" "mixed"
        else
            assert_file "$GAME_DIR/mixin-after.txt" "original"
        fi
    fi
    stop_game

    assert_report "JTDL|ok|done"
    assert_report "JTDL|ok|loader"
    assert_no_report "JTDL|error|entrypoint"
    assert_no_report "JTDL|error|mod"
    assert_no_report "JTDL|error|nested"
}

assert_fabric_evidence() {
    local dir="$1"
    assert_file "$dir/fabric-main.txt" "main-ran"
    assert_file "$dir/fabric-client.txt" "client-ran"
    assert_file "$dir/meteor-entrypoint.txt" "meteor-ran"
    assert_absent "$dir/fabric-server.txt"
    assert_file "$dir/resource.txt" "hola-inyectado"
    assert_file "$dir/nested.txt" "nested-ok"
    assert_report "Game class loader: KnotClassLoader"
    assert_report "JTDL|ok|entrypoint"
    assert_report "meteor -> example.ExampleMeteorAddon.onInitialize() invoked"
    assert_report "Fabric entrypoints: invoked=3 failed=0 skipped=1 of 4"
    assert_report "ExamplePreLaunch.onPreLaunch() was recognized but skipped"
    assert_report "Skipping server entrypoint example.ExampleServer"
    assert_absent "$dir/prelaunch-constructor.txt"
    assert_absent "$dir/prelaunch-callback.txt"


    assert_report "entrypoints=5, mixins=1, bundledJars=1"
    assert_report "JTDL|ok|nested"


    assert_report "JTDL|ok|mixin"
}

run_route agent inject-jar "$AGENT" "$WORK/game-classes" Main "$FABRIC_JAR"
assert_fabric_evidence "$WORK/agent"
assert_report "JTDL|ok|system-search"
assert_no_report "JTDL|detail|system-search"
assert_report "received changed bytecode from the active Mixin transformer"
assert_file "$WORK/agent/mixin-before.txt" "original"
assert_file "$WORK/agent/mixin-after.txt" "mixed"


assert_file "$WORK/agent/loader.txt" "AppClassLoader"

run_route native inject "$LIBRARY" "$WORK/game-classes" Main "$FABRIC_JAR"
assert_fabric_evidence "$WORK/native"
assert_report "JTDL|detail|system-search"
assert_no_report "JTDL|ok|system-search"
assert_report "this attach route cannot retransform already loaded target classes"
assert_file "$WORK/native/mixin-before.txt" "original"
assert_file "$WORK/native/mixin-after.txt" "original"


assert_file "$WORK/native/loader.txt" "KnotClassLoader"

run_route forge inject-jar "$AGENT" "$WORK/forge-classes" ForgeMain "$FORGE_JAR"
assert_file "$WORK/forge/forge-bus.txt" "ctor-bus"
assert_file "$WORK/forge/forge-container.txt" "exampleforge"
assert_report "Game class loader: LaunchClassLoader"
assert_report "Forge/NeoForge metadata, id=exampleforge"
assert_report "Forge/NeoForge mod classes instantiated: 2 of 2"
assert_report "Resolved the mod event bus from net.minecraftforge.fml.javafmlmod.FMLJavaModLoadingContext"
assert_report "does not get Forge's own discovery"

echo "INJECTOR JVM TEST OK"
