# Changelog

All notable changes to JAR to Dynamic Library are recorded here.

## [0.2.1] - 2026-11-03

### Added
- An automatic GitHub release check that offers **Update now** and **Later**
  when a newer stable release is available.
- The supplied hero banner and social network icons in the desktop interface.

### Changed
- Improved the rotating landscape carousel with smoother transitions, subtle
  motion, and corrected image framing.
- Updated the visible application name to **JAR to Dynamic Library** and
  refined interface colors, contrast, and controls.

## [0.2.0] - 2026-11-02

### Added
- A redesigned, single-window desktop GUI with Build, Injector, and Changelog tabs.
- Cross-platform Java process discovery through Linux `/proc`, Windows WMI/CIM
  (`java.exe` and `javaw.exe`), and macOS `ps`. Discovery no longer depends on
  the launcher's Java runtime or a system JDK being on `PATH`.
- Minecraft process classification using vanilla game arguments and common
  Fabric, Quilt, Forge, NeoForge, and LaunchWrapper entrypoints; all other Java
  processes remain manually selectable for custom launchers.
- An explicit confirmation dialog showing the selected process, JAR, and
  injection route; the JDK Attach API is used only for the actual attach.
- Two attach routes: a Java agent JAR (with `Instrumentation`) and a native
  library (`Agent_OnAttach`).
- Runtime inspection of the selected mod JAR, including resources, Fabric/Quilt
  metadata, entrypoints, nested JARs, Forge/NeoForge metadata, and Mixin configs.
- Startup injection setup for Prism Launcher/MultiMC: Fabric uses the loader's
  `fabric.addMods` property; Forge and NeoForge use the early-start adapter
  before their mod scan, with existing instance JVM arguments backed up.
- Startup injection now scans common launcher folders and presents detected
  instances for selection, with manual folder browsing for custom launchers.
- The changelog uses a compact, wrapped layout to fit more release history in
  the available window height.
- A late-injection confirmation that explains loader lifecycle and
  retransformation limitations; the main content and action controls are
  centered in the GUI.
- Structured injector diagnostics written by the target JVM and streamed into
  the GUI log.
- Cross-platform build targets for Windows, Linux, and macOS (x86_64 and
  Apple Silicon where supported).

### Fixed
- The GUI now correlates reports with a unique per-attach session token, so an
  old report cannot be mistaken for the current run even when file size and
  timestamps match.
- A failed JNI environment acquisition no longer consumes the duplicate-call
  guard; threads attached by the injector are detached on setup failure.
- Different mod JARs can be attached to one JVM, while the same archive is
  detected and not loaded twice.
- The header wraps on narrow windows instead of clipping navigation and social
  controls.
- The GitHub button now points to the JTDL repository.
- A failed OS process scan now returns the GUI to its idle state instead of
  leaving the Injector tab stuck on its scanning indicator.
- The GUI no longer preselects an unrelated Java application when no Minecraft
  launch signature is found; custom JVMs remain available for manual selection.

### Changed
- Injection runs through the target JVM's Attach API instead of relying only
  on the original pre-packaged class injection path.
- The application waits for the injector's final report before marking the
  operation complete, preventing a second attach from starting while the GUI
  is still reading the current run.
- Process discovery no longer fails just because `java`/`javac` is not installed
  globally or a launcher uses a bundled Java runtime.

## [0.1.0] - 2026-10-23

### Initial release baseline
- First desktop GUI for selecting a Minecraft mod JAR and target operating
  systems.
- Initial builder that packaged mod classes into native `.dll`, `.so`, and
  `.dylib` libraries.
- Initial JNI/Java injector for loading the packaged mod classes into a JVM.
- Baseline Forge, NeoForge, and Fabric support, with the original limitations
  around dynamic class loading, Fabric metadata, and Mixin configuration.
