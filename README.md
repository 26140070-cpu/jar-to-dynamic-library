# JAR to Dynamic Library

JAR to Dynamic Library packages a Minecraft mod `.jar` into a native library
(`.dll`, `.so`, or `.dylib`) that loads the mod into a Minecraft JVM.

## For players

Download the desktop package for your system from the repository's **Actions**
page, or run the GUI from a project checkout. The desktop package includes the
project files needed to build libraries, but Rust/Cargo must be installed:

1. Open the latest successful **Desktop packages** workflow run.
2. Download the matching artifact: Windows `.exe`, Linux `.AppImage`, or macOS
   `.dmg`.
3. Extract the complete downloaded artifact; keep the `project` folder beside
   the executable (or inside the macOS app).
4. Install Rust 1.88 or newer and Cargo from [rustup.rs](https://rustup.rs).
   On Windows, install the Visual Studio C++ Build Tools as well.
5. Launch the app, choose your mod `.jar`, select the platform(s) you need, and
   click **Build libraries**.
6. Find the resulting library in a `<mod-name>-libraries` folder next to the
   selected JAR.

The GUI builds the native library, not a Minecraft launcher or installer. You
must load that library into the Minecraft process separately, using a method
appropriate for your setup (for example, `System.load`, a Java agent, or
`dlopen` on Unix-like systems). The desktop package includes the Java injector,
so a JDK is not needed at runtime.

### Build from source

The GUI requires Rust 1.88 or newer and a JDK 8 or newer. The JDK is used to
compile the bundled Java injector. Install Rust from [rustup.rs](https://rustup.rs)
and a JDK such as [Eclipse Temurin](https://adoptium.net/temurin/), then run:

```shell
cargo run --features gui --bin jar-to-dynamic-library-gui
```

On Windows, building for Windows requires the MSVC toolchain and the Visual
Studio C++ Build Tools. Cross-compiling to Windows from Linux or macOS requires
LLVM/Clang; the GUI installs `cargo-xwin` automatically and downloads the
Windows SDK the first time. Cross-compiling to macOS or Linux uses Zig, which
the GUI installs on first use. These cross-compilation steps need an internet
connection.

## Desktop packages

The **Desktop packages** GitHub Actions workflow builds the GUI and publishes
platform-specific workflow artifacts:

| Artifact | Platform | Contents |
| --- | --- | --- |
| `jar-to-dynamic-library-windows-x64` | Windows x64 | `.exe` GUI and `project` folder |
| `jar-to-dynamic-library-linux-x64` | Linux x64 | AppImage with the GUI and `project` folder |
| `jar-to-dynamic-library-macos-arm64` | macOS Apple Silicon | Disk image containing the app and `project` folder |
| `jar-to-dynamic-library-macos-x64` | macOS Intel | Disk image containing the app and `project` folder |

Download these from **Actions → Desktop packages → latest successful run →
Artifacts** and extract the complete artifact before launching it. Rust/Cargo
is required to build libraries. These desktop packages are separate from the
`.dll`, `.so`, and `.dylib` files produced by the app.

## Build a library from the command line

Requirements:

- Rust 1.75 or newer
- A JDK 8 or newer (`javac` must be on `PATH`, or set `JAVAC` / `JAVA_HOME`)

Provide a mod JAR as `input.jar` in the repository root, or set `INPUT_JAR`:

```shell
INPUT_JAR=/path/to/your-mod.jar cargo build --release
```

The native library is written to `target/release/`. To cross-compile, install
the Rust target and the required linker/toolchain first:

```shell
rustup target add <target-triple>
cargo build --release --target <target-triple>
```

For release builds, the JAR is required. A build without a mod would create a
library that silently does nothing.

## Supported mods and limitations

Support is inherited from the upstream project and has not yet been re-verified
against real Forge installations:

- Forge, NeoForge, and Fabric mods are supported.
- Mods that need `URLClassLoader` for dynamic work are not supported.
- Forge / NeoForge mods must initialize in a no-argument `@Mod` constructor.
  Constructors that only accept `IEventBus` or `ModContainer` are not supported.
- Fabric entrypoint classes must implement `ModInitializer`,
  `ClientModInitializer`, or `DedicatedServerModInitializer` and have a public
  no-argument constructor. Their `onInitialize*` method is called.
- `fabric.mod.json` is not read and mixins are not applied.

The injector works with Java 8 and Java 16 or newer. It defines classes through
JNI and does not require `--add-opens`. On Windows, the library stays loaded
after injection because the Java side calls back into the native library.

## Troubleshooting

- **No JAR selected / release build fails:** select a valid `.jar` file in the
  GUI, or provide it as `INPUT_JAR` for a command-line build.
- **`javac` not found:** install a JDK 8 or newer and add its `bin` directory to
  `PATH`, or set `JAVA_HOME` / `JAVAC`.
- **Windows cross-build asks for Clang:** install LLVM and make sure `clang` is
  available on `PATH`. `cargo-xwin` also downloads the Windows SDK on first use.
- **A target fails while other targets succeed:** check the build log for that
  target. Cross-compilation requires internet access on first run and may need
  additional system toolchains.
- **The library does not load a mod:** make sure the library matches the
  operating system and architecture of the running Minecraft process, and that
  the mod meets the limitations above.

## Development

```shell
cargo fmt --check
cargo clippy --release -- -D warnings
tests/e2e/run.sh
E2E_JAVA=/path/to/java tests/e2e/run.sh
```

The end-to-end test needs a JDK and runs on Linux, macOS, and BSD. CI builds
Windows x64, Linux x64, macOS arm64, and macOS x64 libraries.

## Simple tutorial

For a step-by-step guide to building a library with GitHub Actions, see the
[manual for users](docs/for-dummies/FOR_DUMMIES.md).
