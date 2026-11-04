# JAR to Dynamic Library

JAR to Dynamic Library (JTDL) is a desktop tool for Minecraft Java mods. It can
build native libraries from a mod JAR, or try to load the original JAR into a
running Java game. For the best compatibility, configure a mod to load when the
game starts instead of injecting it after startup.

## Download and install the app

Download the desktop package for your operating system from this project's
**Actions → Desktop packages** artifacts:

| Operating system | Package |
| --- | --- |
| Windows 64-bit | `jar-to-dynamic-library-windows-x64.exe` |
| Linux 64-bit | `jar-to-dynamic-library-linux-x64.AppImage` |
| macOS Apple Silicon | `jar-to-dynamic-library-macos-arm64.dmg` |
| macOS Intel | `jar-to-dynamic-library-macos-x64.dmg` |

These are portable packages; the desktop app does not need to be installed
through a Minecraft launcher. GitHub Actions artifacts are temporary downloads
and expire after 14 days; they are not permanent release downloads.

- **Windows:** run the downloaded `.exe`.
- **Linux:** make the AppImage executable, then run it. For example:

  ```sh
  chmod +x jar-to-dynamic-library-linux-x64.AppImage
  ./jar-to-dynamic-library-linux-x64.AppImage
  ```

  To add JTDL to the desktop app menu and show its icon on the taskbar, extract
  the package artifact and run the included installer script:

  ```sh
  chmod +x jar-to-dynamic-library-linux-x64.AppImage
  ./install-linux-desktop.sh "$PWD/jar-to-dynamic-library-linux-x64.AppImage" \
    "$PWD/jar-to-dynamic-library-icon.png"
  ```

  This registers a per-user desktop entry with the same application ID as the
  window. It works with Wayland and X11 desktop environments. Sign out and back
  in, or restart the desktop shell, if the taskbar continues showing a generic
  icon.
- **macOS:** open the `.dmg`, then drag **JAR to Dynamic Library.app** to
  Applications and launch it from there. Choose the Apple Silicon or Intel
  package to match the Mac.

The packages above are workflow artifacts and may be available only to people
with access to the project. If you have the source code instead, build and run
the GUI with the instructions under [Build from source](#build-from-source).

JAR to Dynamic Library checks GitHub for a newer stable release at startup. If
one is available, choose **Update now** to open its download page, or **Later**
to continue using the current version.

## Choose what you want to do

### Build a native library

1. Open **Build** and select the mod `.jar`.
2. Select the operating systems and processor architectures you need.
3. Choose **Build libraries**.

JTDL creates a `<mod-name>-libraries` folder beside the selected JAR. A native
library must match the operating system and processor architecture of the Java
game process. Building a library does not install or load the mod into
Minecraft; use the game's mod folder or one of the injection options below to
load it.

### Load a mod when the game starts (recommended)

1. Open **Injector**, select the original mod `.jar`, and choose **Inject at
   startup**.
2. Select the Minecraft instance to configure. JTDL searches common locations
   for Minecraft, Prism Launcher, MultiMC, PolyMC, TLauncher, and SKlauncher
   instances. If yours is not listed, browse to its game or instance folder.
3. Follow the confirmation and launcher-specific instructions. Start the game
   from the configured instance after setup.

Startup setup is loader-specific:

- **Fabric:** JTDL adds the selected JAR to Fabric's startup mod candidates
  using `fabric.addMods`.
- **Forge and NeoForge:** JTDL prepares an early-start Java agent and gives it
  the mod before the loader scans mods.

JTDL edits Prism Launcher and MultiMC instance settings directly and saves a
backup before changing them. For other launchers, it shows the JVM arguments
you need to add to that instance's Java/JVM arguments setting. Paste each
argument as a separate argument, keep your existing arguments, and launch the
same profile you configured. The setting's name and location vary by launcher.
JTDL can find instances in supported common folders, but it does not edit every
launcher automatically.

If a launcher does not expose custom JVM arguments, use the standard approach:
put the mod in that instance's `mods` folder and start Minecraft normally.

### Inject into a game that is already running (experimental)

Choose **Inject now (late)**, select the running Minecraft process, review the
warning, and confirm. This attaches to an already-started JVM. Fabric, Forge,
or NeoForge have already completed mod discovery and startup callbacks, so the
mod may only load partially or fail. Use startup setup or the normal `mods`
folder when possible.

## Launcher and operating-system requirements

- JTDL can detect Minecraft processes started by common and custom launchers;
  unsupported launchers may still work if their game process is visible.
- Automatic instance discovery searches known locations, not every custom
  launcher installation. Use the folder browser when an instance is missing.
- Prism Launcher and MultiMC settings can be configured automatically.
  Other launchers require adding the displayed JVM arguments manually.
- On Linux, run JTDL and Minecraft as the same user and in the same filesystem
  environment. Flatpak, Snap, or other sandboxing can prevent process attachment
  or access to the game's temporary files. For late injection, both processes
  may need to share the same `/tmp` and mount namespace.
- On Windows and macOS, allow the app to inspect or attach to the game process
  if the operating system requests permission. Avoid running one program with
  elevated privileges while the other runs without them.
- Use a JDK, not only a JRE. The Java Attach API and `javac` are required for
  attaching and preparing some loader adapters. The target game must be a Java
  Edition Minecraft process.

## Compatibility and safety

Only select mod JARs you trust: loading or attaching a mod runs its code inside
the game process.

Late injection is inherently limited. It cannot replay Fabric, Forge, or
NeoForge startup phases, `preLaunch` entrypoints, access wideners, or changes
that must be made before classes load. The Java-agent route can ask the active
Mixin transformer to retransform already-loaded classes, but the JVM rejects
structural changes such as adding fields, methods, or interfaces. The native
library route cannot retransform loaded classes. Registering a mixin
configuration does not guarantee that any mixin was applied.

The experimental late-Mixin path depends on internal Mixin behavior and has not
been verified against every Minecraft, loader, or Mixin version. Check the
diagnostic report to see whether mod entrypoints ran; a successful JVM attach
alone does not mean the mod loaded successfully.

For regular play, installing the mod in the instance's `mods` folder and
restarting the game remains the most compatible option.

## Logs and troubleshooting

JTDL writes its injector report to:

- **Linux/macOS:** `~/jar-to-dynamic-library-log.txt`
- **Windows:** `%USERPROFILE%\jar-to-dynamic-library-log.txt`

In the GUI, **Diagnostics** shows the selected JAR, Java runtime, process, and
expected outputs. The **Changelog** tab and [release history](CHANGELOG.md)
describe app changes.

If late injection on Linux reports `Unable to access the filesystem of the
target process`, the JDK cannot reach the target JVM's temporary directory.
This is commonly caused by different sandbox, container, or mount namespaces,
or by an unshared `/tmp`. Run both programs in the same filesystem environment
and restart Minecraft before retrying. Changing injection routes does not
bypass this Attach API restriction.

For Fabric startup behavior, see the [Fabric Loader discovery
documentation](https://docs.fabricmc.net/develop/loader/) and its
[`fabric.addMods` implementation](https://github.com/FabricMC/fabric-loader/blob/master/src/main/java/net/fabricmc/loader/impl/discovery/ArgumentModCandidateFinder.java).
The Forge/NeoForge startup adapter uses Java's
[instrumentation agent API](https://docs.oracle.com/en/java/javase/21/docs/api/java.instrument/java/lang/instrument/package-summary.html).

## Build from source

Install Rust/Cargo and a JDK, then run:

```sh
cargo run --features gui --bin jar-to-dynamic-library-gui
```

To build an optimized GUI binary:

```sh
cargo build --release --features gui --bin jar-to-dynamic-library-gui
```

On Linux, if you launch the binary directly and your taskbar shows a generic
Wayland/X11 icon, register the desktop entry and icon for your user:

```sh
bash scripts/install-linux-desktop.sh \
  target/release/jar-to-dynamic-library-gui
```

This installer does not require root access. It registers JTDL's icon for
windows whose application ID is `jar-to-dynamic-library-gui`.

The GUI needs Rust 1.88 or newer and Cargo. Building the project also requires a
JDK with `javac`; cross-compiling native libraries may require Zig and
LLVM/Clang. The GUI can offer to install Zig. If you do not want to install Rust
or a JDK, see the [GitHub Actions build guide](docs/for-dummies/FOR_DUMMIES.md)
for building a native library. Name the uploaded mod JAR `input.jar` for that
workflow.
