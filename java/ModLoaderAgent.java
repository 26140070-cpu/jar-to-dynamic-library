package io.jtdl.agent;

import java.io.BufferedReader;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.lang.instrument.Instrumentation;
import java.nio.charset.StandardCharsets;
import java.nio.file.DirectoryStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Properties;
import java.util.Set;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;
import java.util.jar.Manifest;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

public final class ModLoaderAgent {
    private static final String BUNDLE_METADATA = "META-INF/jtdl/bundle.properties";
    private static final String EMBEDDED_MOD = "META-INF/jtdl/embedded-mod.jar";
    private static final long MAX_EMBEDDED_BYTES = 1024L * 1024L * 1024L;
    private static final long MAX_METADATA_BYTES = 1024L * 1024L;
    private static final Pattern TOML_MOD_ID = Pattern.compile("^\\s*modId\\s*=\\s*[\\\"']([^\\\"']+)[\\\"']");
    private static final Pattern LEGACY_MOD_ID = Pattern.compile("\\\"modid\\\"\\s*:\\s*\\\"([^\\\"]+)\\\"", Pattern.CASE_INSENSITIVE);
    private static final Pattern VALID_MOD_ID = Pattern.compile("^[a-z][a-z0-9_]{1,63}$");

    private ModLoaderAgent() { }

    public static void premain(String agentArgs, Instrumentation instrumentation) {
        try {
            extractBeforeModScan(agentArgs);
        } catch (Throwable error) {
            System.err.println("[JTDL] ERROR: loader adapter could not prepare the embedded mod: " + error);
            error.printStackTrace(System.err);
            throw new IllegalStateException("JTDL Forge/NeoForge adapter failed before mod discovery", error);
        }
    }










    public static void agentmain(String agentArgs, Instrumentation instrumentation) {
        System.out.println(
                "[JTDL] Dynamic attach: extracting the embedded mod now. If the loader already "
                        + "finished its mod scan for this run, restart the game to pick it up.");
        try {
            extractBeforeModScan(agentArgs);
        } catch (Throwable error) {
            System.err.println("[JTDL] ERROR: dynamic attach could not prepare the embedded mod: " + error);
            error.printStackTrace(System.err);
        }
    }

    private static void extractBeforeModScan(String agentArgs) throws Exception {
        if (agentArgs == null || agentArgs.trim().isEmpty()) {
            throw new IllegalArgumentException("agent argument must be the absolute path to a .jtdl bundle");
        }
        Path bundle = Paths.get(agentArgs.trim()).toAbsolutePath().normalize();
        if (!Files.isRegularFile(bundle)) {
            throw new IOException("bundle not found: " + bundle);
        }

        Properties metadata = new Properties();
        Path modsDirectory;
        Path temporary = null;
        try (ZipFile archive = new ZipFile(bundle.toFile())) {
            ZipEntry metadataEntry = archive.getEntry(BUNDLE_METADATA);
            if (metadataEntry == null || metadataEntry.getSize() > MAX_METADATA_BYTES) {
                throw new IOException("bundle metadata is missing or exceeds 1 MiB");
            }
            byte[] metadataBytes;
            try (InputStream input = archive.getInputStream(metadataEntry);
                 ByteArrayOutputStream output = new ByteArrayOutputStream()) {
                byte[] buffer = new byte[8192];
                int read;
                while ((read = input.read(buffer)) != -1) {
                    if (output.size() + read > MAX_METADATA_BYTES) {
                        throw new IOException("bundle metadata exceeds 1 MiB");
                    }
                    output.write(buffer, 0, read);
                }
                metadataBytes = output.toByteArray();
            }
            try (InputStream input = new ByteArrayInputStream(metadataBytes)) {
                metadata.load(input);
            }

            String bundleVersion = metadata.getProperty("bundleVersion", "");
            if (!"1".equals(bundleVersion)) {
                throw new IOException("unsupported JTDL bundle version: " + bundleVersion);
            }
            String loader = metadata.getProperty("loader", "").trim().toLowerCase(Locale.ROOT);
            if (!"forge".equals(loader) && !"neoforge".equals(loader)) {
                throw new IOException("bundle loader must be forge or neoforge, not: " + loader);
            }
            String agentLoader = getAgentLoader();
            if (agentLoader == null) {
                throw new IOException("could not read Jtdl-Loader from the adapter-agent manifest");
            }
            if (!agentLoader.equals(loader)) {
                throw new IOException("this is the " + agentLoader + " adapter, but the bundle targets " + loader);
            }

            String embeddedPath = metadata.getProperty("embeddedPath", EMBEDDED_MOD);
            if (!EMBEDDED_MOD.equals(embeddedPath)) {
                throw new IOException("unsupported embedded mod path: " + embeddedPath);
            }
            ZipEntry embedded = archive.getEntry(embeddedPath);
            if (embedded == null || embedded.isDirectory()) {
                throw new IOException("embedded mod JAR is missing from the bundle");
            }
            if (embedded.getSize() < 1 || embedded.getSize() > MAX_EMBEDDED_BYTES) {
                throw new IOException("embedded mod size is invalid or exceeds 1 GiB");
            }

            String configuredModsDirectory = System.getProperty("jtdl.modsDir", "").trim();
            if (configuredModsDirectory.isEmpty()) {
                throw new IOException(
                        "jtdl.modsDir is required. Set -Djtdl.modsDir=/absolute/path/to/your/instance/mods "
                                + "so the agent extracts into the correct Minecraft instance instead of inventing a folder from user.dir");
            }
            modsDirectory = Paths.get(configuredModsDirectory).toAbsolutePath().normalize();
            if (!Files.isDirectory(modsDirectory)) {
                throw new IOException(
                        "jtdl.modsDir does not exist or is not a directory: " + modsDirectory
                                + ". Create or select the instance mods folder first");
            }
            Path instanceRoot = modsDirectory.getParent();
            if (instanceRoot == null || !Files.isDirectory(instanceRoot)) {
                throw new IOException(
                        "jtdl.modsDir must be a 'mods' folder inside an existing Minecraft instance directory: "
                                + modsDirectory);
            }
            String modsLeaf = modsDirectory.getFileName() == null
                    ? ""
                    : modsDirectory.getFileName().toString();
            if (!"mods".equalsIgnoreCase(modsLeaf)) {
                throw new IOException(
                        "jtdl.modsDir must end with a folder named 'mods' (got: " + modsDirectory + ")");
            }

            temporary = Files.createTempFile(modsDirectory, ".jtdl-embedded-", ".tmp");
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            long copied = 0L;
            byte[] buffer = new byte[64 * 1024];
            try (InputStream input = archive.getInputStream(embedded);
                 OutputStream output = Files.newOutputStream(temporary, StandardOpenOption.TRUNCATE_EXISTING)) {
                int read;
                while ((read = input.read(buffer)) != -1) {
                    copied += read;
                    if (copied > MAX_EMBEDDED_BYTES) {
                        throw new IOException("embedded mod exceeds 1 GiB while extracting");
                    }
                    digest.update(buffer, 0, read);
                    output.write(buffer, 0, read);
                }
            }
            if (embedded.getSize() >= 0 && copied != embedded.getSize()) {
                throw new IOException("embedded mod size does not match its ZIP metadata");
            }

            String sha256 = toHex(digest.digest());
            Set<String> expectedModIds = splitModIds(metadata.getProperty("modIds", ""));
            if (expectedModIds.isEmpty()) {
                throw new IOException("bundle has no declared mod IDs; rebuild it from a valid mod JAR");
            }
            for (String id : expectedModIds) {
                if (!VALID_MOD_ID.matcher(id).matches()) {
                    throw new IOException("bundle declares invalid Forge/NeoForge modId: " + id);
                }
            }
            verifyModJar(temporary, loader, expectedModIds);

            String sourceStem = safeStem(metadata.getProperty("sourceFileName", "embedded-mod"));
            Path destination = modsDirectory.resolve("jtdl-" + sourceStem + "-" + sha256.substring(0, 12) + ".jar");
            checkForDuplicateModIds(modsDirectory, destination, loader, expectedModIds);
            if (Files.exists(destination)) {
                String existingHash = sha256(destination);
                if (!sha256.equals(existingHash)) {
                    throw new IOException("destination already exists with different content: " + destination);
                }
                Files.deleteIfExists(temporary);
                temporary = null;
                System.err.println("[JTDL] Embedded mod was already extracted: " + destination);
                return;
            }

            try {
                Files.move(temporary, destination, StandardCopyOption.ATOMIC_MOVE);
            } catch (UnsupportedOperationException | java.nio.file.AtomicMoveNotSupportedException unsupported) {
                Files.move(temporary, destination);
            }
            temporary = null;
            System.err.println("[JTDL] Extracted " + loader + " mod " + join(expectedModIds)
                    + " to " + destination);
            System.err.println("[JTDL] The real " + loader
                    + " loader will now process the mod through its normal startup lifecycle.");
        } finally {
            if (temporary != null) {
                try { Files.deleteIfExists(temporary); } catch (IOException ignored) { }
            }
        }
    }

    private static String getAgentLoader() {
        try {
            Path ownJar = Paths.get(ModLoaderAgent.class.getProtectionDomain().getCodeSource().getLocation().toURI());
            if (!Files.isRegularFile(ownJar)) return null;
            try (JarFile file = new JarFile(ownJar.toFile())) {
                Manifest manifest = file.getManifest();
                return manifest == null ? null : manifest.getMainAttributes().getValue("Jtdl-Loader");
            }
        } catch (Throwable ignored) {
            return null;
        }
    }

    private static void verifyModJar(Path jarPath, String loader, Set<String> expectedIds) throws IOException {
        try (JarFile jar = new JarFile(jarPath.toFile())) {
            JarEntry metadata = "forge".equals(loader)
                    ? firstEntry(jar, "META-INF/mods.toml", "mcmod.info")
                    : jar.getJarEntry("META-INF/neoforge.mods.toml");
            if (metadata == null) {
                throw new IOException("embedded JAR lacks metadata for " + loader + "; it will not be loaded by this adapter");
            }
            Set<String> foundIds = parseModIds(jar, metadata);
            if (foundIds.isEmpty()) {
                throw new IOException("could not read a mod ID from " + metadata.getName());
            }
            for (String id : foundIds) {
                if (!VALID_MOD_ID.matcher(id).matches()) {
                    throw new IOException("embedded JAR contains invalid Forge/NeoForge modId: " + id);
                }
            }
            if (!foundIds.equals(expectedIds)) {
                throw new IOException("bundle metadata does not exactly match the embedded mod metadata; expected "
                        + join(expectedIds) + " but found " + join(foundIds));
            }
        }
    }

    private static JarEntry firstEntry(JarFile jar, String first, String fallback) {
        JarEntry entry = jar.getJarEntry(first);
        return entry != null ? entry : jar.getJarEntry(fallback);
    }

    private static Set<String> parseModIds(JarFile jar, JarEntry entry) throws IOException {
        Set<String> ids = new HashSet<String>();
        String name = entry.getName().toLowerCase(Locale.ROOT);
        if (name.endsWith(".toml")) {
            try (InputStream input = jar.getInputStream(entry);
                 BufferedReader reader = new BufferedReader(new InputStreamReader(input, StandardCharsets.UTF_8))) {
                String line;
                boolean inModsTable = false;
                long chars = 0L;
                while ((line = reader.readLine()) != null) {
                    chars += line.length();
                    if (chars > MAX_METADATA_BYTES) throw new IOException("mod metadata exceeds 1 MiB");
                    String trimmed = line.trim();
                    if (trimmed.startsWith("#")) continue;
                    if ("[[mods]]".equals(trimmed)) {
                        inModsTable = true;
                        continue;
                    }
                    if (trimmed.startsWith("[")) {
                        inModsTable = false;
                    }
                    if (inModsTable) {
                        Matcher matcher = TOML_MOD_ID.matcher(line);
                        if (matcher.find()) ids.add(matcher.group(1).trim());
                    }
                }
            }
        } else {
            StringBuilder content = new StringBuilder();
            try (InputStream input = jar.getInputStream(entry);
                 BufferedReader reader = new BufferedReader(new InputStreamReader(input, StandardCharsets.UTF_8))) {
                String line;
                while ((line = reader.readLine()) != null) {
                    content.append(line).append('\n');
                    if (content.length() > MAX_METADATA_BYTES) throw new IOException("legacy mod metadata exceeds 1 MiB");
                }
            }
            Matcher matcher = LEGACY_MOD_ID.matcher(content.toString());
            while (matcher.find()) ids.add(matcher.group(1).trim());
        }
        return ids;
    }

    private static void checkForDuplicateModIds(Path modsDirectory, Path destination, String loader,
                                                Set<String> requestedIds) throws IOException {
        List<Path> candidates = new ArrayList<Path>();
        collectJarCandidates(modsDirectory, candidates, 0);
        for (Path candidate : candidates) {
            if (candidate.equals(destination)) {
                continue;
            }
            try {
                try (JarFile jar = new JarFile(candidate.toFile())) {
                    JarEntry metadata = "forge".equals(loader)
                            ? firstEntry(jar, "META-INF/mods.toml", "mcmod.info")
                            : jar.getJarEntry("META-INF/neoforge.mods.toml");
                    if (metadata == null) continue;
                    Set<String> installedIds = parseModIds(jar, metadata);
                    for (String id : requestedIds) {
                        if (installedIds.contains(id)) {
                            throw new IOException("mod ID '" + id + "' already exists in " + candidate
                                    + "; remove the original/duplicate JAR before using this bundle");
                        }
                    }
                }
            } catch (IOException duplicate) {
                if (duplicate.getMessage() != null && duplicate.getMessage().contains("already exists")) {
                    throw duplicate;
                }
                System.err.println("[JTDL] Skipping unreadable mods entry during duplicate scan: "
                        + candidate + " (" + duplicate.getMessage() + ")");
            } catch (Throwable foreign) {
                System.err.println("[JTDL] Skipping unreadable mods entry during duplicate scan: "
                        + candidate + " (" + foreign + ")");
            }
        }
    }

    private static void collectJarCandidates(Path directory, List<Path> out, int depth) throws IOException {
        if (depth > 1) {
            return;
        }
        try (DirectoryStream<Path> entries = Files.newDirectoryStream(directory)) {
            for (Path candidate : entries) {
                try {
                    if (Files.isDirectory(candidate)) {
                        collectJarCandidates(candidate, out, depth + 1);
                        continue;
                    }
                    if (!Files.isRegularFile(candidate)) {
                        continue;
                    }
                    String name = candidate.getFileName().toString().toLowerCase(Locale.ROOT);
                    if (name.endsWith(".jar")) {
                        out.add(candidate);
                    }
                } catch (Throwable ignored) {
                }
            }
        }
    }

    private static Set<String> splitModIds(String raw) {
        Set<String> ids = new HashSet<String>();
        for (String id : raw.split(",")) {
            String candidate = id.trim();
            if (!candidate.isEmpty()) ids.add(candidate);
        }
        return ids;
    }

    private static String join(Set<String> ids) {
        List<String> sorted = new ArrayList<String>(ids);
        java.util.Collections.sort(sorted);
        StringBuilder out = new StringBuilder();
        for (String id : sorted) {
            if (out.length() > 0) out.append(", ");
            out.append(id);
        }
        return out.toString();
    }

    private static String safeStem(String raw) {
        String source = raw == null ? "embedded-mod" : raw;
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < source.length() && out.length() < 70; i++) {
            char c = source.charAt(i);
            if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
                    || (c >= '0' && c <= '9') || c == '-' || c == '_' || c == '.') {
                out.append(c);
            } else {
                out.append('-');
            }
        }
        String value = out.toString();
        if (value.isEmpty() || ".".equals(value) || "..".equals(value)) return "embedded-mod";
        return value;
    }

    private static String sha256(Path filePath) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-256");
        byte[] buffer = new byte[64 * 1024];
        try (InputStream input = Files.newInputStream(filePath)) {
            int read;
            while ((read = input.read(buffer)) != -1) digest.update(buffer, 0, read);
        }
        return toHex(digest.digest());
    }

    private static String toHex(byte[] bytes) {
        StringBuilder result = new StringBuilder(bytes.length * 2);
        for (byte value : bytes) result.append(String.format(Locale.ROOT, "%02x", value & 0xff));
        return result.toString();
    }
}
