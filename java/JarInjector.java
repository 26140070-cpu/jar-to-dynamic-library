import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.io.InputStream;
import java.io.PrintWriter;
import java.lang.annotation.Annotation;
import java.lang.instrument.Instrumentation;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.net.MalformedURLException;
import java.net.URL;
import java.security.CodeSource;
import java.security.ProtectionDomain;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.Enumeration;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.jar.JarFile;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

 
























public final class JarInjector {

    private static final String LOG_FILE =
            System.getProperty("user.home") + File.separator + "jar-to-dynamic-library-log.txt";
    private static final String FORK = "JTDL";

    private static PrintWriter fileLog;
    private static final Object LOG_LOCK = new Object();
    private static final Set<String> STARTED_JARS = Collections.synchronizedSet(new HashSet<String>());

    private JarInjector() { }

     

    public static void agentmain(String arguments, Instrumentation instrumentation) {
        start(arguments, instrumentation, "agentmain (JDK attach)");
    }

    public static void premain(String arguments, Instrumentation instrumentation) {
        start(arguments, instrumentation, "premain (-javaagent)");
    }

    public static void premain(String arguments) {
        start(arguments, null, "premain (-javaagent, no Instrumentation)");
    }

     
    public static void inject(String options) {
        String arguments = options != null && options.indexOf('=') >= 0
                ? options : "jar=" + options;
        start(arguments, null, "Agent_OnAttach (native library)");
    }

    private static void start(String arguments, Instrumentation instrumentation, String channel) {
        String sessionId = option(arguments, "session");
        if (sessionId != null) {
            report("detail", "session", sessionId);
        }
        final String jarPath = option(arguments, "jar");
        if (jarPath == null || jarPath.trim().isEmpty()) {
            report("error", "boot", "No 'jar=' option was supplied through " + channel
                    + "; nothing to load. Options were: " + arguments);
            report("error", "done", "The injector stopped because no mod JAR was supplied.");
            return;
        }
        final File jarFile = new File(jarPath);
        if (!jarFile.isFile()) {
            report("error", "jar", "The mod JAR does not exist or is not readable: " + jarFile);
            report("error", "done", "The injector stopped because the mod JAR is unavailable.");
            return;
        }
        String jarKey;
        try {
            jarKey = new File(jarPath).getCanonicalPath();
        } catch (IOException ignored) {
            jarKey = new File(jarPath).getAbsolutePath();
        }
        if (!STARTED_JARS.add(jarKey)) {
            report("warn", "boot", "This mod JAR was already injected into this JVM; skipping duplicate attach.");
            report("warn", "done", "The duplicate archive was not loaded a second time.");
            return;
        }
        report("info", "boot", "JarInjector started through " + channel + " with jar=" + jarPath);
        Thread worker = new Thread(new Runnable() {
            public void run() {
                try {
                    injectArchive(new File(jarPath), instrumentation);
                } catch (Throwable error) {
                    report("error", "fatal", "The injector crashed: " + error, error);
                    report("error", "done", "The injector stopped after an unrecoverable error.");
                } finally {
                    closeLog();
                }
            }
        }, "jtdl-injector");
        worker.setDaemon(true);
        worker.start();
    }

     

    private static void injectArchive(File jar, Instrumentation instrumentation) {
        if (!jar.isFile()) {
            report("error", "jar", "The mod JAR does not exist or is not readable: " + jar);
            return;
        }
        ClassLoader gameLoader = detectGameClassLoader();
        report(gameLoader == null ? "warn" : "ok", "loader",
                gameLoader == null
                        ? "No Knot / LaunchClassLoader / ModClassLoader found in any thread; using the "
                          + "platform loader as parent, so mod code cannot see Minecraft classes."
                        : "Game class loader: " + gameLoader.getClass().getName());

        JarClassLoader loader = new JarClassLoader(
                gameLoader != null ? gameLoader : platformLoader(), "jtdl-" + jar.getName());

         
         
         
        boolean systemSearchAvailable = false;
        if (instrumentation != null) {
            try {
                instrumentation.appendToSystemClassLoaderSearch(new JarFile(jar));
                systemSearchAvailable = true;
                report("ok", "system-search", "Added " + jar + " to the system class loader search.");
            } catch (Throwable error) {
                report("warn", "system-search", "appendToSystemClassLoaderSearch failed: " + error, error);
            }
        } else {
            report("detail", "system-search",
                    "No Instrumentation available on this route; the archive is served by the "
                    + "injector's own class loader instead.");
        }

         
        try {
            loader.addArchive(jar);
        } catch (IOException error) {
            report("error", "classloader", "Cannot open the mod JAR as a zip: " + error, error);
            report("error", "done", "The injector stopped because the mod archive could not be opened.");
            return;
        }

        JarManifest manifest = JarManifest.read(jar, loader);
        report("info", "manifest", manifest.describe());

         
        loadNestedJars(jar, manifest, loader, instrumentation);

         
        registerWithGameLoader(gameLoader, loader, systemSearchAvailable);

         
        boolean success;
        if (manifest.isFabricLike() || canLoad(loader, "net.fabricmc.api.ModInitializer")) {
            success = injectFabric(loader, manifest, gameLoader);
        } else {
            success = injectForge(loader, manifest, gameLoader);
        }

        int transformedMixinTargets = injectMixins(loader, manifest, instrumentation);

        String resourceSample = manifest.resourceCount + " archive entries are reachable through "
                + "getResource/getResources (assets, configs, lang files).";
        if (success || transformedMixinTargets > 0) {
            report("ok", "done", "Injection finished. " + resourceSample);
        } else {
            report("error", "done", "Injection did not run any mod entrypoint. " + resourceSample);
        }
    }

     

     




    private static boolean injectFabric(ClassLoader loader, JarManifest manifest, ClassLoader gameLoader) {
        String environment = envType(loader);
        if (environment == null) {
            environment = guessEnvironment(loader);
        }
        boolean client = environment == null || "CLIENT".equals(environment);
        boolean server = environment == null || "SERVER".equals(environment);
        if ("client".equals(manifest.environment)) {
            server = false;
        } else if ("server".equals(manifest.environment)) {
            client = false;
        }
        report("info", "fabric", "EnvType=" + environment + " declaredEnvironment=" + manifest.environment
                + " -> running client entrypoints=" + client + ", server entrypoints=" + server);

        List<String[]> targets = new ArrayList<String[]>();
        for (int i = 0; i < manifest.entrypoints.size(); i++) {
            String[] entry = (String[]) manifest.entrypoints.get(i);
            String group = entry[0];
            if ("client".equals(group) && !client) {
                report("detail", "fabric", "Skipping client entrypoint " + entry[1] + " (this is a server)");
                continue;
            }
            if ("server".equals(group) && !server) {
                report("detail", "fabric", "Skipping server entrypoint " + entry[1] + " (this is a client)");
                continue;
            }
            targets.add(entry);
        }
        if (targets.isEmpty()) {
            report("warn", "fabric",
                    "fabric.mod.json declares no usable entrypoint; scanning the archive for "
                    + "ModInitializer/ClientModInitializer/DedicatedServerModInitializer instead.");
            targets = scanForInitializers(loader, manifest);
        }

        int invoked = 0;
        int failed = 0;
        int skipped = 0;
        for (int i = 0; i < targets.size(); i++) {
            String group = targets.get(i)[0];
            String className = targets.get(i)[1];
            try {
                Class type = loader.loadClass(className);
                if ("preLaunch".equals(group) || "pre_launch".equals(group)) {
                    Method callback = findCallback(loader, type, group);
                    if (callback == null) {
                        report("warn", "entrypoint", type.getName()
                                + " is declared under \"preLaunch\" but has no onPreLaunch() callback.");
                    } else {
                        report("warn", "entrypoint", type.getName()
                                + ".onPreLaunch() was recognized but skipped: preLaunch runs before the game "
                                + "starts, and this JVM is already running. Restart the game to run it safely.");
                    }
                    skipped++;
                    continue;
                }
                Object instance = instantiate(type, manifest, loader, gameLoader);
                String callback = invokeEntrypoint(loader, type, instance, group, manifest);
                if (callback != null) {
                    invoked++;
                    report("ok", "entrypoint", group + " -> " + className + "." + callback + "() invoked");
                }
            } catch (Throwable error) {
                failed++;
                report("error", "entrypoint", "Entrypoint " + group + " -> " + className
                        + " failed: " + rootCause(error), error);
            }
        }
        if (invoked == 0 && targets.isEmpty()) {
            report("error", "fabric",
                    "No Fabric entrypoint could be found. Add \"entrypoints\" to fabric.mod.json "
                    + "or make sure the mod classes implement net.fabricmc.api initializers.");
            return false;
        }
        report(invoked > 0 ? "ok" : "error", "fabric",
                "Fabric entrypoints: invoked=" + invoked + " failed=" + failed + " skipped=" + skipped
                + " of " + targets.size());
        return invoked > 0;
    }

    private static List<String[]> scanForInitializers(ClassLoader loader, JarManifest manifest) {
        List<String[]> found = new ArrayList<String[]>();
        ZipFile archive = manifest.archive();
        if (archive == null) {
            return found;
        }
        Enumeration<? extends ZipEntry> entries = archive.entries();
        while (entries.hasMoreElements()) {
            String name = entries.nextElement().getName();
            if (!name.endsWith(".class") || name.indexOf('$') >= 0) {
                continue;
            }
            String className = name.substring(0, name.length() - 6).replace('/', '.');
            Class type;
            try {
                type = loader.loadClass(className);
            } catch (Throwable error) {
                continue;
            }
            if (type.isInterface() || Modifier.isAbstract(type.getModifiers())) {
                continue;
            }
            String group = initializerGroup(type);
            if (group != null) {
                found.add(new String[] { group, className });
            }
        }
        return found;
    }

     
    private static String initializerGroup(Class type) {
        String best = null;
        int bestRank = 0;
        for (Class candidate = type; candidate != null; candidate = candidate.getSuperclass()) {
            Class[] interfaces = candidate.getInterfaces();
            for (int i = 0; i < interfaces.length; i++) {
                int rank = rankOf(interfaces[i].getName());
                if (rank > bestRank) {
                    bestRank = rank;
                    best = groupOf(interfaces[i].getName());
                }
            }
        }
        return best;
    }

    private static int rankOf(String iface) {
        if ("net.fabricmc.api.ModInitializer".equals(iface)) {
            return 3;
        }
        if ("net.fabricmc.api.ClientModInitializer".equals(iface)) {
            return 4;
        }
        if ("net.fabricmc.api.DedicatedServerModInitializer".equals(iface)) {
            return 4;
        }
        if (iface.endsWith("PreLaunchInitializer")) {
            return 5;
        }
        return 0;
    }

    private static String groupOf(String iface) {
        if ("net.fabricmc.api.ClientModInitializer".equals(iface)) {
            return "client";
        }
        if ("net.fabricmc.api.DedicatedServerModInitializer".equals(iface)) {
            return "server";
        }
        if (iface.endsWith("PreLaunchInitializer")) {
            return "preLaunch";
        }
        return "main";
    }

     





    private static String invokeEntrypoint(ClassLoader loader, Class type, Object instance, String group,
                                           JarManifest manifest) throws Exception {
        Method callback = findCallback(loader, type, group);
        if (callback == null) {
            report("warn", "entrypoint", type.getName() + " was listed under \"" + group
                    + "\" but no matching callback method was found; the real Fabric loader would use "
                    + "its own dispatcher for this group.");
            return null;
        }
        Object[] arguments = argumentsFor(loader, callback, manifest, group);
        if (arguments == null) {
            report("warn", "entrypoint", type.getName() + "." + callback.getName()
                    + "() needs a " + callback.getParameterTypes()[0].getName()
                    + " instance, which only Fabric Loader can build; entrypoint skipped.");
            return null;
        }
        callback.setAccessible(true);
        try {
            callback.invoke(instance, arguments);
        } catch (InvocationTargetException error) {
            Throwable cause = error.getCause() == null ? error : error.getCause();
            throw new Exception("The entrypoint body threw " + cause, cause);
        }
        return callback.getName();
    }

    private static Method findCallback(ClassLoader loader, Class type, String group) {
        String[] preferred = callbackNames(group);
        List methods = implementedMethods(type);
        for (int p = 0; p < preferred.length; p++) {
            for (int i = 0; i < methods.size(); i++) {
                Method method = (Method) methods.get(i);
                if (method.getName().equals(preferred[p])) {
                    return method;
                }
            }
        }
        for (int i = 0; i < methods.size(); i++) {
            Method method = (Method) methods.get(i);
            if (method.getName().startsWith("onInitialize")) {
                return method;
            }
        }
        Method[] publicMethods = type.getMethods();
        for (int p = 0; p < preferred.length; p++) {
            for (int i = 0; i < publicMethods.length; i++) {
                Method method = publicMethods[i];
                if (method.getName().equals(preferred[p]) && !Modifier.isStatic(method.getModifiers())) {
                    return method;
                }
            }
        }
        if ("main".equals(group) || "client".equals(group) || "server".equals(group)
                || "preLaunch".equals(group) || "pre_launch".equals(group)) {
            return null;
        }
        for (int i = 0; i < publicMethods.length; i++) {
            Method method = publicMethods[i];
            if (method.getName().startsWith("onInitialize") && !Modifier.isStatic(method.getModifiers())) {
                return method;
            }
        }
        return null;
    }

    private static String[] callbackNames(String group) {
        if ("client".equals(group)) {
            return new String[] { "onInitializeClient" };
        }
        if ("server".equals(group)) {
            return new String[] { "onInitializeServer" };
        }
        if ("preLaunch".equals(group) || "pre_launch".equals(group)) {
            return new String[] { "onPreLaunch" };
        }
        return new String[] { "onInitialize" };
    }

     
    private static List implementedMethods(Class type) {
        List out = new ArrayList();
        Set seen = new LinkedHashSet();
        for (Class candidate = type; candidate != null; candidate = candidate.getSuperclass()) {
            Class[] interfaces = candidate.getInterfaces();
            for (int i = 0; i < interfaces.length; i++) {
                String name = interfaces[i].getName();
                if (!seen.add(name)) {
                    continue;
                }
                if (!name.startsWith("net.fabricmc") && !isModInterface(type, interfaces[i])) {
                    continue;
                }
                Method[] methods = interfaces[i].getDeclaredMethods();
                for (int m = 0; m < methods.length; m++) {
                    if (!Modifier.isStatic(methods[m].getModifiers())) {
                        out.add(methods[m]);
                    }
                }
            }
        }
        return out;
    }

    private static boolean isModInterface(Class type, Class iface) {
        return iface.isAssignableFrom(type) && iface.getName().startsWith("org.quiltmc");
    }

    private static Object[] argumentsFor(ClassLoader loader, Method callback, JarManifest manifest, String group)
            throws Exception {
        Class[] parameters = callback.getParameterTypes();
        if (parameters.length == 0) {
            return new Object[0];
        }
        if (parameters.length == 1 && parameters[0].getName().endsWith("ModContainer")) {
            Object container = fabricModContainer(loader, manifest.modId);
            return container == null ? null : new Object[] { container };
        }
        return null;
    }

    private static Object fabricModContainer(ClassLoader loader, String modId) {
        if (modId == null) {
            return null;
        }
        try {
            Class fabricLoader = loader.loadClass("net.fabricmc.loader.api.FabricLoader");
            Object instance = fabricLoader.getMethod("getInstance").invoke(null);
            Object optional = fabricLoader.getMethod("getModContainer", String.class)
                    .invoke(instance, modId);
            if (optional instanceof java.util.Optional) {
                java.util.Optional present = (java.util.Optional) optional;
                return present.isPresent() ? present.get() : null;
            }
            Method isPresent = optional.getClass().getMethod("isPresent");
            Method get = optional.getClass().getMethod("get");
            return Boolean.TRUE.equals(isPresent.invoke(optional)) ? get.invoke(optional) : null;
        } catch (Throwable error) {
            report("detail", "fabric", "Could not resolve the Fabric ModContainer for " + modId + ": " + error);
            return null;
        }
    }

    private static String envType(ClassLoader loader) {
        try {
            Class fabricLoader = loader.loadClass("net.fabricmc.loader.api.FabricLoader");
            Object instance = fabricLoader.getMethod("getInstance").invoke(null);
            Object environment = instance.getClass().getMethod("getEnvironmentType").invoke(instance);
            return String.valueOf(environment);
        } catch (Throwable error) {
            report("detail", "environment", "FabricLoader.getInstance().getEnvironmentType() is not "
                    + "reachable (" + error + "); the environment is guessed from the game classes below.");
            return null;
        }
    }

     





    private static String guessEnvironment(ClassLoader loader) {
        boolean client = canLoad(loader, "net.minecraft.client.Minecraft");
        boolean server = canLoad(loader, "net.minecraft.server.MinecraftServer");
        if (client && !server) {
            return "CLIENT";
        }
        if (server && !client) {
            return "SERVER";
        }
        return null;
    }

     

    private static boolean injectForge(ClassLoader loader, JarManifest manifest, ClassLoader gameLoader) {
        Class modAnnotation = loadFirst(loader, new String[] {
                "net.neoforged.fml.common.Mod",
                "cpw.mods.fml.common.Mod",
                "net.minecraftforge.fml.common.Mod" });
        if (modAnnotation == null) {
            report("error", "forge", "No Forge/NeoForge @Mod annotation is visible to the game loader; "
                    + "this JAR does not look like a Forge mod either.");
            return false;
        }
        List classes = modAnnotatedClasses(loader, manifest, modAnnotation);
        if (classes.isEmpty()) {
            report("error", "forge", "No class annotated with @Mod was found inside the JAR.");
            return false;
        }
        int invoked = 0;
        for (int i = 0; i < classes.size(); i++) {
            Class type = (Class) classes.get(i);
            try {
                Object instance = instantiate(type, manifest, loader, gameLoader);
                invoked++;
                report("ok", "mod", "Instanced " + type.getName() + " -> " + instance.getClass().getName());
            } catch (Throwable error) {
                report("error", "mod", "Could not instance " + type.getName() + ": " + rootCause(error), error);
            }
        }
        report(invoked > 0 ? "ok" : "error", "forge",
                "Forge/NeoForge mod classes instantiated: " + invoked + " of " + classes.size()
                + ". Note that a mod attached after boot does not get Forge's own discovery, "
                + "dependency resolution or mod-bus phases.");
        return invoked > 0;
    }

    private static List modAnnotatedClasses(ClassLoader loader, JarManifest manifest, Class modAnnotation) {
        List out = new ArrayList();
        Set names = new LinkedHashSet();
        for (int i = 0; i < manifest.forgeModClasses.size(); i++) {
            names.add(manifest.forgeModClasses.get(i));
        }
        ZipFile archive = manifest.archive();
        if (archive != null) {
            Enumeration<? extends ZipEntry> entries = archive.entries();
            while (entries.hasMoreElements()) {
                String name = entries.nextElement().getName();
                if (name.endsWith(".class") && name.indexOf('$') < 0) {
                    names.add(name.substring(0, name.length() - 6).replace('/', '.'));
                }
            }
        }
        for (Object value : names) {
            Class type;
            try {
                type = loader.loadClass((String) value);
            } catch (Throwable error) {
                continue;
            }
            if (type.getAnnotation(modAnnotation) != null) {
                out.add(type);
            }
        }
        return out;
    }

     




    private static Object instantiate(Class type, JarManifest manifest, ClassLoader loader, ClassLoader gameLoader)
            throws Exception {
        try {
            Constructor constructor = type.getConstructor();
            constructor.setAccessible(true);
            return constructor.newInstance();
        } catch (NoSuchMethodException missing) {
             
        } catch (InvocationTargetException error) {
            Throwable cause = error.getCause() == null ? error : error.getCause();
            throw new Exception("The no-arg constructor of " + type.getName() + " threw " + cause, cause);
        }
        Constructor[] candidates = type.getDeclaredConstructors();
        Exception last = null;
        for (int i = 0; i < candidates.length; i++) {
            Constructor candidate = candidates[i];
            try {
                candidate.setAccessible(true);
                Object[] arguments = forgeArguments(loader, candidate.getParameterTypes(), manifest);
                if (arguments == null) {
                    continue;
                }
                return candidate.newInstance(arguments);
            } catch (Throwable error) {
                last = new Exception(candidate + " -> " + rootCause(error), error instanceof Exception
                        ? (Exception) error : null);
            }
        }
        throw new Exception("Unsupported constructor for " + type.getName()
                + ". This injector can supply the event bus and mod container through reflection, "
                + "but the signature was not resolvable: " + (last == null ? "no usable constructor" : last
                .getMessage()), last);
    }

    private static Object[] forgeArguments(ClassLoader loader, Class[] parameters, JarManifest manifest) {
        Object[] arguments = new Object[parameters.length];
        for (int i = 0; i < parameters.length; i++) {
            arguments[i] = forgeArgument(loader, parameters[i], manifest);
            if (arguments[i] == null && parameters[i].isPrimitive()) {
                return null;
            }
        }
        return arguments;
    }

    private static Object forgeArgument(ClassLoader loader, Class parameter, JarManifest manifest) {
        String name = parameter.getName();
        if (name.endsWith("IEventBus") || name.endsWith("EventBus")) {
            return forgeEventBus(loader);
        }
        if (name.endsWith("ModContainer")) {
            return forgeModContainer(loader, manifest.modId);
        }
        return null;
    }

    private static Object forgeEventBus(ClassLoader loader) {
        String[][] probes = {
                { "net.minecraftforge.fml.javafmlmod.FMLJavaModLoadingContext", "getModEventBus" },
                { "net.neoforged.fml.javafmlmod.FMLJavaModLoadingContext", "getModEventBus" },
                { "net.minecraftforge.fml.ModLoader", "getEventBus" },
                { "net.neoforged.fml.ModLoader", "getEventBus" },
        };
        for (int i = 0; i < probes.length; i++) {
            try {
                Class owner = loader.loadClass(probes[i][0]);
                Method accessor = owner.getMethod(probes[i][1]);
                Object target = Modifier.isStatic(accessor.getModifiers())
                        ? null
                        : ownerTarget(accessor.getDeclaringClass());
                Object bus = accessor.invoke(target);
                if (bus != null) {
                    report("detail", "forge", "Resolved the mod event bus from " + probes[i][0]);
                    return bus;
                }
            } catch (Throwable ignored) {
                 
            }
        }
        return null;
    }

     




    private static Object ownerTarget(Class owner) throws Exception {
        String[] accessors = { "get", "getInstance", "instance" };
        Exception last = null;
        for (int i = 0; i < accessors.length; i++) {
            try {
                Method find = owner.getMethod(accessors[i]);
                if (!Modifier.isStatic(find.getModifiers())) {
                    continue;
                }
                Object value = find.invoke(null);
                if (value != null) {
                    return value;
                }
            } catch (NoSuchMethodException missing) {
                last = missing;
            } catch (InvocationTargetException thrown) {
                last = new Exception(accessors[i] + "() threw " + thrown.getCause(), thrown.getCause());
            }
        }
        if (last instanceof NoSuchMethodException) {
            throw new NoSuchMethodException(owner.getName() + " exposes no static get()/getInstance()");
        }
        throw last;
    }

    private static Object forgeModContainer(ClassLoader loader, String modId) {
        if (modId == null) {
            return null;
        }
        String[] owners = { "net.minecraftforge.fml.ModList", "net.neoforged.fml.ModList" };
        for (int i = 0; i < owners.length; i++) {
            try {
                Class list = loader.loadClass(owners[i]);
                Object instance = list.getMethod("get").invoke(null);
                Object container = list.getMethod("getModContainerById", String.class)
                        .invoke(instance, modId);
                Object present = container.getClass().getMethod("isPresent").invoke(container);
                if (!Boolean.TRUE.equals(present)) {
                    continue;
                }
                return container.getClass().getMethod("get").invoke(container);
            } catch (Throwable ignored) {
                 
            }
        }
        return null;
    }

     

    private static int injectMixins(ClassLoader loader, JarManifest manifest, Instrumentation instrumentation) {
        if (manifest.mixinConfigs.isEmpty()) {
            return 0;
        }
        if (manifest.accessWidener != null) {
            report("warn", "mixin", "The mod ships an access widener (" + manifest.accessWidener
                    + "); the loader applies it during its own startup, so mixins that rely on the widened "
                    + "members can still fail when the mod is injected after the game booted.");
        }
        Class mixins = loadFirst(loader, new String[] { "org.spongepowered.asm.mixin.Mixins" });
        Method addConfiguration = null;
        if (mixins != null) {
            try {
                addConfiguration = mixins.getMethod("addConfiguration", String.class);
            } catch (Throwable error) {
                addConfiguration = null;
            }
        }
        if (addConfiguration == null) {
            report("warn", "mixin", manifest.mixinConfigs.size() + " mixin configuration(s) are declared, but "
                    + "org.spongepowered.asm.mixin.Mixins is not visible to the game loader: no mixin was "
                    + "applied. Use the early-startup adapter kit for mods that depend on mixins.");
            return 0;
        }

        String environment = envType(loader);
        int registered = 0;
        int retransformed = 0;
        int transformed = 0;
        for (int i = 0; i < manifest.mixinConfigs.size(); i++) {
            String path = manifest.mixinConfigs.get(i);
            Object parsed = parseJson(manifest.readText(path));
            if (!(parsed instanceof Map)) {
                report("warn", "mixin", "The mixin configuration " + path
                        + " is missing from the archive or is not a JSON object.");
                continue;
            }
            Map config = (Map) parsed;
            String packageName = jsonText(config, "package");
            String plugin = jsonText(config, "plugin");
            List names = mixinClassNames(config, packageName, environment);
            String via = registerMixinConfig(loader, addConfiguration, path);
            if (via == null) {
                continue;
            }
            registered++;
            MixinTransformerAccess mixinTransformer = null;
            try {
                mixinTransformer = activeMixinTransform(loader);
            } catch (Throwable error) {
                report("warn", "mixin", "Registered " + path
                        + ", but could not prepare the active Mixin engine to select the late config: "
                        + rootCause(error), error);
            }
            report("ok", "mixin", "Registered " + path + " (" + names.size() + " mixin class(es)"
                    + (packageName == null ? "" : ", package " + packageName)
                    + ", required=" + booleanOf(config, "required", true)
                    + ") through " + via
                    + (mixinTransformer == null
                            ? "; late Mixin selection could not be verified."
                            : "; the active Mixin engine can now select this late config."));
            if (plugin != null) {
                report("warn", "mixin", "The configuration declares the mixin plugin " + plugin
                        + "; the active Mixin engine must initialize it, and its behavior may depend on "
                        + "startup state that is no longer available.");
            }
            if (mixinTransformer != null && instrumentation != null) {
                int[] result = retransformTargets(loader, names, instrumentation, mixinTransformer);
                retransformed += result[0];
                transformed += result[1];
            } else if (mixinTransformer != null) {
                report("info", "mixin", path + " is queued in the active Mixin engine; this attach route "
                        + "cannot retransform already loaded target classes.");
            }
        }
        if (registered > 0) {
            report("info", "mixin", registered + " of " + manifest.mixinConfigs.size()
                    + " configuration(s) registered; " + retransformed
                    + " already loaded target class(es) retransformed; " + transformed
                    + " received changed bytecode from the active Mixin transformer.");
        }
        return transformed;
    }

     






    private static String registerMixinConfig(ClassLoader loader, Method addConfiguration, String path) {
        Object processor = null;
        try {
            processor = activeMixinProcessor(loader);
        } catch (Throwable error) {
            report("warn", "mixin", "Could not synchronize late config registration with the active "
                    + "Mixin processor: " + rootCause(error), error);
        }
        if (processor != null) {
            synchronized (processor) {
                return registerMixinConfigLocked(loader, addConfiguration, path);
            }
        }
        return registerMixinConfigLocked(loader, addConfiguration, path);
    }

    private static String registerMixinConfigLocked(ClassLoader loader, Method addConfiguration, String path) {
        List failures = new ArrayList();
        try {
            addConfiguration.invoke(null, path);
            return "Mixins.addConfiguration(String)";
        } catch (Throwable error) {
            failures.add("Mixins.addConfiguration(String) -> " + rootCause(error));
        }

        Class environmentClass = loadFirst(loader,
                new String[] { "org.spongepowered.asm.mixin.MixinEnvironment" });
        if (environmentClass == null) {
            report("error", "mixin", "No mixin API accepted " + path + ": " + failures.get(0)
                    + " (full trace in " + LOG_FILE + ").");
            return null;
        }
        Method current;
        try {
            current = environmentClass.getMethod("getCurrentEnvironment");
        } catch (Throwable error) {
            report("error", "mixin", "MixinEnvironment has no getCurrentEnvironment(): " + rootCause(error)
                    + " (full trace in " + LOG_FILE + ").");
            return null;
        }

         
        try {
            Method withLoader = environmentClass.getMethod("addConfiguration", String.class, ClassLoader.class);
            withLoader.invoke(current.invoke(null), path, loader);
            return "MixinEnvironment.addConfiguration(String, ClassLoader)";
        } catch (Throwable error) {
            failures.add("MixinEnvironment.addConfiguration(String, ClassLoader) -> " + rootCause(error));
        }

         
        try {
            Method plain = environmentClass.getMethod("addConfiguration", String.class);
            plain.invoke(current.invoke(null), path);
            return "MixinEnvironment.addConfiguration(String)";
        } catch (Throwable error) {
            failures.add("MixinEnvironment.addConfiguration(String) -> " + rootCause(error));
        }

        StringBuilder reason = new StringBuilder();
        for (int i = 0; i < failures.size(); i++) {
            reason.append(i == 0 ? "" : " | ").append(failures.get(i));
        }
        report("error", "mixin", "No Mixin API accepted " + path + ". " + reason
                + " (full stack traces in " + LOG_FILE + "). This configuration was not registered; "
                + "load the mod through the game's normal startup instead.");
        return null;
    }

     
    private static List mixinClassNames(Map config, String packageName, String environment) {
        List names = new ArrayList();
        addMixinNames(names, packageName, config.get("mixins"));
        Object scoped = "server".equalsIgnoreCase(environment) ? config.get("server") : config.get("client");
        addMixinNames(names, packageName, scoped);
        return names;
    }

    private static void addMixinNames(List names, String packageName, Object value) {
        if (!(value instanceof List)) {
            return;
        }
        List entries = (List) value;
        for (int i = 0; i < entries.size(); i++) {
            Object entry = entries.get(i);
            String simple = null;
            if (entry instanceof String) {
                simple = (String) entry;
            } else if (entry instanceof Map) {
                simple = jsonText((Map) entry, "value");
            }
            if (simple == null || simple.length() == 0) {
                continue;
            }
            String qualified = packageName == null || simple.indexOf('.') >= 0
                    ? simple : packageName + "." + simple;
            if (!names.contains(qualified)) {
                names.add(qualified);
            }
        }
    }

    private static boolean booleanOf(Map map, String key, boolean fallback) {
        Object value = map.get(key);
        return value instanceof Boolean ? ((Boolean) value).booleanValue() : fallback;
    }

     





    private static int[] retransformTargets(ClassLoader loader, List mixinClasses,
                                            Instrumentation instrumentation,
                                            MixinTransformerAccess mixinTransformer) {
        if (mixinClasses.isEmpty() || !instrumentation.isRetransformClassesSupported()) {
            return new int[] { 0, 0 };
        }
        Set<Class> previouslyLoaded = new HashSet<Class>(
                Arrays.asList(instrumentation.getAllLoadedClasses()));
        List targets = new ArrayList();
        for (int i = 0; i < mixinClasses.size(); i++) {
            Class[] found = mixinTargetClasses(loader, (String) mixinClasses.get(i));
            for (int j = 0; j < found.length; j++) {
                if (found[j] != null && previouslyLoaded.contains(found[j]) && !targets.contains(found[j])
                        && instrumentation.isModifiableClass(found[j])) {
                    targets.add(found[j]);
                }
            }
        }
        if (targets.isEmpty()) {
            report("info", "mixin", "No already-loaded modifiable target classes were found; future class "
                    + "definitions will use the selected Mixin configuration through the game loader.");
            return new int[] { 0, 0 };
        }
        final Set<Class> targetSet = Collections.synchronizedSet(new HashSet<Class>(targets));
        final Set<Class> changed = Collections.synchronizedSet(new HashSet<Class>());
        final List<String> failures = Collections.synchronizedList(new ArrayList<String>());
        java.lang.instrument.ClassFileTransformer bridge = new java.lang.instrument.ClassFileTransformer() {
            public byte[] transform(ClassLoader definingLoader, String className, Class classBeingRedefined,
                                    ProtectionDomain domain, byte[] classfileBuffer) {
                if (classBeingRedefined == null || !targetSet.contains(classBeingRedefined)) {
                    return null;
                }
                String name = className.replace('/', '.');
                try {
                    byte[] output;
                    synchronized (mixinTransformer.processor) {
                        output = (byte[]) mixinTransformer.method.invoke(
                                mixinTransformer.instance, name, name, classfileBuffer);
                    }
                    if (output != null && !Arrays.equals(output, classfileBuffer)) {
                        changed.add(classBeingRedefined);
                    }
                    return output;
                } catch (Throwable error) {
                    failures.add(name + " -> " + rootCause(error));
                    return null;
                }
            }
        };
        boolean installed = false;
        try {
            instrumentation.addTransformer(bridge, true);
            installed = true;
            instrumentation.retransformClasses((Class[]) targets.toArray(new Class[targets.size()]));
            if (!failures.isEmpty()) {
                report("warn", "mixin", "The active Mixin transformer failed for "
                        + failures.size() + " retransformed target(s): " + failures.get(0));
            }
            int changedCount = changed.size();
            report(changedCount > 0 ? "ok" : "warn", "mixin",
                    "Retransformed " + targets.size() + " target class(es) through the active Mixin "
                    + "transformer; changed bytecode accepted by the JVM for " + changedCount
                    + ". The JVM rejects structural changes to loaded classes.");
            return new int[] { targets.size(), changedCount };
        } catch (Throwable error) {
            report("warn", "mixin", "Late Mixin retransformation failed for " + targets.size()
                    + " target class(es): " + rootCause(error)
                    + ". Use the game's normal mod loading and restart if the mixin adds fields, methods, "
                    + "interfaces, or otherwise requires startup-time transformation.", error);
            return new int[] { 0, 0 };
        } finally {
            if (installed && !instrumentation.removeTransformer(bridge)) {
                report("warn", "mixin", "The temporary Mixin retransformation bridge could not be removed.");
            }
        }
    }

    private static MixinTransformerAccess activeMixinTransform(ClassLoader loader) throws Exception {
        Class mixinsApi = loader.loadClass("org.spongepowered.asm.mixin.Mixins");
        Method getUnvisitedCount = mixinsApi.getMethod("getUnvisitedCount");
        int unvisited = ((Number) getUnvisitedCount.invoke(null)).intValue();
        if (unvisited == 0) {
            return null;
        }
        Object processor = activeMixinProcessor(loader);
        Field transformedCountField = findField(processor.getClass(), "transformedCount");
        if (transformedCountField == null || transformedCountField.getType() != Integer.TYPE) {
            throw new IllegalStateException("active Mixin processor has no compatible selection gate");
        }
        transformedCountField.setAccessible(true);
        synchronized (processor) {
            int previousCount = transformedCountField.getInt(processor);
            if (previousCount != 0) {
                transformedCountField.setInt(processor, 0);
                report("info", "mixin", "Reset the active Mixin processor's late-config selection gate so "
                        + "it can prepare newly registered configurations.");
            }
        }
        Class transformerApi = loader.loadClass("org.spongepowered.asm.mixin.transformer.IMixinTransformer");
        Method method = transformerApi.getMethod("transformClassBytes", String.class, String.class, byte[].class);
        Object active = activeMixinTransformer(loader);
        return new MixinTransformerAccess(active, processor, method);
    }

    private static Object activeMixinProcessor(ClassLoader loader) throws Exception {
        Object active = activeMixinTransformer(loader);
        Field processorField = findField(active.getClass(), "processor");
        if (processorField == null) {
            throw new IllegalStateException("active Mixin transformer has no processor field");
        }
        processorField.setAccessible(true);
        Object processor = processorField.get(active);
        if (processor == null) {
            throw new IllegalStateException("active Mixin transformer has no initialized processor");
        }
        return processor;
    }

    private static Object activeMixinTransformer(ClassLoader loader) throws Exception {
        Class environmentClass = loader.loadClass("org.spongepowered.asm.mixin.MixinEnvironment");
        Object environment = environmentClass.getMethod("getCurrentEnvironment").invoke(null);
        Object active = environmentClass.getMethod("getActiveTransformer").invoke(environment);
        if (active == null) {
            throw new IllegalStateException("Mixin has no active transformer");
        }
        Class transformerApi = loader.loadClass("org.spongepowered.asm.mixin.transformer.IMixinTransformer");
        if (!transformerApi.isInstance(active)) {
            throw new IllegalStateException("active transformer does not implement IMixinTransformer: "
                    + active.getClass().getName());
        }
        return active;
    }

    private static Field findField(Class type, String name) {
        for (Class current = type; current != null; current = current.getSuperclass()) {
            try {
                return current.getDeclaredField(name);
            } catch (NoSuchFieldException ignored) {
            }
        }
        return null;
    }

    private static final class MixinTransformerAccess {
        final Object instance;
        final Object processor;
        final Method method;

        MixinTransformerAccess(Object instance, Object processor, Method method) {
            this.instance = instance;
            this.processor = processor;
            this.method = method;
        }
    }

     
    private static Class[] mixinTargetClasses(ClassLoader loader, String className) {
        try {
            Class type = loader.loadClass(className);
            Annotation[] annotations = type.getAnnotations();
            for (int i = 0; i < annotations.length; i++) {
                Class kind = annotations[i].annotationType();
                if (!"org.spongepowered.asm.mixin.Mixin".equals(kind.getName())) {
                    continue;
                }
                Object value = kind.getMethod("value").invoke(annotations[i]);
                if (value instanceof Class[]) {
                    return (Class[]) value;
                }
            }
        } catch (Throwable ignored) {
             
        }
        return new Class[0];
    }

     

    private static void loadNestedJars(File jar, JarManifest manifest, JarClassLoader loader,
                                       Instrumentation instrumentation) {
        Set<String> paths = new LinkedHashSet<String>();
        for (int i = 0; i < manifest.nestedJars.size(); i++) {
            paths.add((String) manifest.nestedJars.get(i));
        }
        ZipFile archive = manifest.archive();
        if (archive != null) {
            Enumeration<? extends ZipEntry> entries = archive.entries();
            while (entries.hasMoreElements()) {
                String name = entries.nextElement().getName();
                if (name.toLowerCase().startsWith("meta-inf/jars/") && name.endsWith(".jar")) {
                    paths.add(name);
                }
            }
        }
        if (paths.isEmpty()) {
            return;
        }
        File directory = new File(System.getProperty("java.io.tmpdir"),
                "jtdl-nested-" + (manifest.modId == null ? jar.getName() : manifest.modId));
        if (!directory.isDirectory() && !directory.mkdirs()) {
            report("warn", "nested", "Could not create " + directory + "; bundled jars stay unloaded.");
            return;
        }
        int loaded = 0;
        for (String path : paths) {
            InputStream stream = archive == null ? null : manifest.open(path);
            if (stream == null) {
                report("warn", "nested", "Declared nested jar is missing from the archive: " + path);
                continue;
            }
            File target = new File(directory, new File(path).getName());
            try {
                copy(stream, target);
                loader.addArchive(target);
                if (instrumentation != null) {
                    try {
                        instrumentation.appendToSystemClassLoaderSearch(new JarFile(target));
                    } catch (Throwable error) {
                        report("detail", "nested", "Could not add " + target + " to the system search: " + error);
                    }
                }
                loaded++;
                report("ok", "nested", "Loaded bundled jar " + path + " -> " + target);
            } catch (Throwable error) {
                report("error", "nested", "Failed to extract/load " + path + ": " + error, error);
            } finally {
                close(stream);
            }
        }
        report("detail", "nested", loaded + " of " + paths.size() + " bundled jar(s) are on the classpath.");
    }

     

     







    private static void registerWithGameLoader(ClassLoader gameLoader, JarClassLoader loader,
                                               boolean systemSearchAvailable) {
        if (gameLoader == null) {
            return;
        }
        Method addUrl = findAddUrl(gameLoader.getClass());
        if (addUrl == null) {
            report(systemSearchAvailable ? "detail" : "warn", "loader",
                    "The game loader (" + gameLoader.getClass().getName()
                    + ") exposes no addURL; the archive "
                    + (systemSearchAvailable
                            ? "is still available through the Java system class loader search."
                            : "remains reachable through the injector's own class loader.")
                    + " Use the early-load adapter kit if the game must register the mod before startup.");
            return;
        }
        int ok = 0;
        List urls = loader.urls();
        for (int i = 0; i < urls.size(); i++) {
            try {
                addUrl.invoke(gameLoader, urls.get(i));
                ok++;
            } catch (Throwable error) {
                report("detail", "loader", "addURL(" + urls.get(i) + ") was rejected: " + error);
            }
        }
        report(ok == urls.size() ? "ok" : "warn", "loader",
                "Registered " + ok + "/" + urls.size() + " archive(s) with the game loader through "
                + addUrl.getDeclaringClass().getName() + "#addURL.");
    }

    private static Method findAddUrl(Class type) {
        for (Class candidate = type; candidate != null && candidate != ClassLoader.class;
                candidate = candidate.getSuperclass()) {
            Method[] methods = candidate.getDeclaredMethods();
            for (int i = 0; i < methods.length; i++) {
                if (!"addURL".equals(methods[i].getName()) || methods[i].getParameterTypes().length != 1) {
                    continue;
                }
                if (!methods[i].getParameterTypes()[0].equals(URL.class)) {
                    continue;
                }
                try {
                    methods[i].setAccessible(true);
                    return methods[i];
                } catch (Throwable error) {
                     
                }
            }
        }
        return null;
    }

    private static ClassLoader detectGameClassLoader() {
        ClassLoader fallback = null;
        Set threads = Thread.getAllStackTraces().keySet();
        for (Object value : threads) {
            ClassLoader context = ((Thread) value).getContextClassLoader();
            if (context == null) {
                continue;
            }
            String name = context.getClass().getName();
            if (name == null) {
                continue;
            }
            if (isKnotLoader(name)) {
                return context;
            }
            if (fallback == null && isForgeLikeLoader(name)) {
                fallback = context;
            }
            if (fallback == null && canLoad(context, "net.minecraft.client.Minecraft")) {
                fallback = context;
            }
        }
        return fallback;
    }

    private static boolean isKnotLoader(String name) {
        return name.contains("Knot");
    }

    private static boolean isForgeLikeLoader(String name) {
        return name.contains("LaunchClassLoader")
                || name.contains("RelaunchClassLoader")
                || name.contains("TransformingClassLoader")
                || name.contains("ModuleClassLoader")
                || name.contains("ModClassLoader");
    }

    private static ClassLoader platformLoader() {
        ClassLoader loader = ClassLoader.getSystemClassLoader();
        try {
            Object getName = loader.getClass().getMethod("getName").invoke(loader);
            if (!"platform".equals(String.valueOf(getName)) && !"app".equals(String.valueOf(getName))) {
                return loader.getParent() != null ? loader.getParent() : loader;
            }
        } catch (Throwable ignored) {
             
        }
        return loader;
    }

    private static boolean canLoad(ClassLoader loader, String className) {
        if (loader == null) {
            return false;
        }
        try {
            Class.forName(className, false, loader);
            return true;
        } catch (Throwable error) {
            return false;
        }
    }

    private static Class loadFirst(ClassLoader loader, String[] candidates) {
        for (int i = 0; i < candidates.length; i++) {
            try {
                return loader.loadClass(candidates[i]);
            } catch (Throwable ignored) {
                 
            }
        }
        return null;
    }

     

    private static String option(String arguments, String key) {
        if (arguments == null) {
            return null;
        }
        String prefix = key + "=";
        String[] parts = arguments.split(";");
        for (int i = 0; i < parts.length; i++) {
            String part = parts[i].trim();
            if (part.startsWith(prefix)) {
                return part.substring(prefix.length()).trim();
            }
        }
        if (parts.length == 1 && arguments.indexOf('=') < 0) {
            return arguments.trim();
        }
        return null;
    }

    private static void report(String level, String component, String message) {
        report(level, component, message, null);
    }

    private static void report(String level, String component, String message, Throwable error) {
        String line = FORK + "|" + level + "|" + component + "|" + message;
        synchronized (LOG_LOCK) {
            PrintWriter writer = log();
            if (writer != null) {
                writer.println(line);
                writer.flush();
                if (error != null) {
                    error.printStackTrace(writer);
                    writer.flush();
                }
            }
        }
        if ("error".equals(level) || "warn".equals(level)) {
            System.err.println("[JTDL] " + line);
            if (error != null) {
                error.printStackTrace();
            }
        }
    }

    private static PrintWriter log() {
        if (fileLog == null) {
            try {
                fileLog = new PrintWriter(LOG_FILE, "UTF-8");
            } catch (Throwable error) {
                System.err.println("[JTDL] error|log|cannot write " + LOG_FILE + ": " + error);
                return null;
            }
        }
        return fileLog;
    }

    private static void closeLog() {
        synchronized (LOG_LOCK) {
            if (fileLog != null) {
                fileLog.close();
                fileLog = null;
            }
        }
    }

    private static String rootCause(Throwable error) {
        Throwable cursor = error;
        while (cursor.getCause() != null && cursor.getCause() != cursor) {
            cursor = cursor.getCause();
        }
        String text = cursor.getClass().getName() + ": " + cursor.getMessage();
        StackTraceElement[] trace = cursor.getStackTrace();
        if (trace.length > 0) {
            text = text + " at " + trace[0];
        }
        return text;
    }

    private static void copy(InputStream source, File target) throws IOException {
        java.io.FileOutputStream out = null;
        try {
            out = new java.io.FileOutputStream(target);
            byte[] buffer = new byte[16_384];
            int read;
            while ((read = source.read(buffer)) > 0) {
                out.write(buffer, 0, read);
            }
        } finally {
            if (out != null) {
                try {
                    out.close();
                } catch (IOException ignored) {
                     
                }
            }
        }
    }

    private static void close(InputStream stream) {
        if (stream != null) {
            try {
                stream.close();
            } catch (IOException ignored) {
                 
            }
        }
    }

     

     







    static final class JarClassLoader extends ClassLoader {
        private final List archives = new ArrayList();
        private final List urls = new ArrayList();
        private final String label;

        JarClassLoader(ClassLoader parent, String label) {
            super(parent);
            this.label = label;
        }

        void addArchive(File file) throws IOException {
            URL url = file.toURI().toURL();
            archives.add(new ZipFile(file));
            urls.add(url);
        }

        List urls() {
            return Collections.unmodifiableList(urls);
        }

        int archiveCount() {
            return archives.size();
        }

        protected Class findClass(String name) throws ClassNotFoundException {
            String path = name.replace('.', '/') + ".class";
            for (int i = 0; i < archives.size(); i++) {
                ZipFile archive = (ZipFile) archives.get(i);
                ZipEntry entry = archive.getEntry(path);
                if (entry == null) {
                    continue;
                }
                byte[] bytes;
                try {
                    bytes = readAll(archive.getInputStream(entry));
                } catch (IOException error) {
                    throw new ClassNotFoundException("Cannot read " + name + " from " + archive + ": " + error);
                }
                URL origin = (URL) urls.get(i);
                ProtectionDomain domain = new ProtectionDomain(
                        new CodeSource(origin, (java.security.cert.Certificate[]) null), null);
                return defineClass(name, bytes, 0, bytes.length, domain);
            }
            throw new ClassNotFoundException(name + " (searched " + archives.size() + " archive(s) of " + label + ")");
        }

        protected URL findResource(String name) {
            for (int i = 0; i < archives.size(); i++) {
                ZipFile archive = (ZipFile) archives.get(i);
                if (archive.getEntry(name) != null) {
                    try {
                        return new URL("jar:" + urls.get(i) + "!/" + name);
                    } catch (MalformedURLException error) {
                        return null;
                    }
                }
            }
            return null;
        }

        protected Enumeration findResources(String name) {
            List found = new ArrayList();
            for (int i = 0; i < archives.size(); i++) {
                ZipFile archive = (ZipFile) archives.get(i);
                if (archive.getEntry(name) == null) {
                    continue;
                }
                try {
                    found.add(new URL("jar:" + urls.get(i) + "!/" + name));
                } catch (MalformedURLException ignored) {
                     
                }
            }
            return Collections.enumeration(found);
        }

        InputStream openResource(String name) {
            for (int i = 0; i < archives.size(); i++) {
                ZipFile archive = (ZipFile) archives.get(i);
                ZipEntry entry = archive.getEntry(name);
                if (entry != null) {
                    try {
                        return archive.getInputStream(entry);
                    } catch (IOException ignored) {
                        return null;
                    }
                }
            }
            return null;
        }

        private static byte[] readAll(InputStream stream) throws IOException {
            try {
                ByteArrayOutputStream buffer = new ByteArrayOutputStream();
                byte[] chunk = new byte[16_384];
                int read;
                while ((read = stream.read(chunk)) > 0) {
                    buffer.write(chunk, 0, read);
                }
                return buffer.toByteArray();
            } finally {
                stream.close();
            }
        }

        public String toString() {
            return "JarClassLoader[" + label + ", " + archives.size() + " archive(s)]";
        }
    }

     

     






    static final class JarManifest {
        final String jarPath;
        String modId;
        String modVersion;
        String environment = "*";
        final List entrypoints = new ArrayList();
        final List<String> mixinConfigs = new ArrayList<String>();
        final List<String> nestedJars = new ArrayList<String>();
        final List<String> forgeModClasses = new ArrayList<String>();
        String accessWidener;
        boolean fabricMetadata;
        boolean quiltMetadata;
        int resourceCount;
        private ZipFile archive;

        JarManifest(String jarPath) {
            this.jarPath = jarPath;
        }

        boolean isFabricLike() {
            return fabricMetadata || quiltMetadata || !entrypoints.isEmpty();
        }

        String describe() {
            StringBuilder builder = new StringBuilder();
            builder.append(fabricMetadata ? "Fabric" : quiltMetadata ? "Quilt" : "Forge/NeoForge")
                    .append(" metadata, id=").append(modId)
                    .append(" version=").append(modVersion)
                    .append(", entrypoints=").append(entrypoints.size())
                    .append(", mixins=").append(mixinConfigs.size())
                    .append(", bundledJars=").append(nestedJars.size())
                    .append(accessWidener == null ? "" : ", accessWidener=" + accessWidener);
            return builder.toString();
        }

        ZipFile archive() {
            return archive;
        }

        InputStream open(String path) {
            if (archive == null) {
                return null;
            }
            ZipEntry entry = archive.getEntry(path);
            if (entry == null) {
                entry = archive.getEntry(path.startsWith("/") ? path.substring(1) : path);
            }
            try {
                return entry == null ? null : archive.getInputStream(entry);
            } catch (IOException error) {
                return null;
            }
        }

        String readText(String path) {
            InputStream stream = open(path);
            if (stream == null) {
                return null;
            }
            try {
                return new String(readUtf8(stream), "UTF-8");
            } catch (IOException error) {
                return null;
            } finally {
                close(stream);
            }
        }

        static JarManifest read(File jar, JarClassLoader loader) {
            JarManifest manifest = new JarManifest(jar.getAbsolutePath());
            ZipFile archive;
            try {
                archive = new ZipFile(jar);
            } catch (IOException error) {
                report("error", "manifest", "Cannot open the JAR as a zip: " + error, error);
                return manifest;
            }
            manifest.archive = archive;
            Enumeration<? extends ZipEntry> entries = archive.entries();
            while (entries.hasMoreElements()) {
                ZipEntry entry = entries.nextElement();
                if (entry.isDirectory()) {
                    continue;
                }
                manifest.resourceCount++;
                String name = entry.getName();
                String lower = name.toLowerCase();
                if ("fabric.mod.json".equals(lower)) {
                    manifest.fabricMetadata = true;
                    manifest.readFabric(archive, entry);
                } else if ("quilt.mod.json".equals(lower)) {
                    manifest.quiltMetadata = true;
                    manifest.readQuilt(archive, entry);
                } else if ("meta-inf/mods.toml".equals(lower) || "meta-inf/neoforge.mods.toml".equals(lower)) {
                    manifest.readModsToml(characters(archive, entry));
                } else if (lower.endsWith(".mixins.json")) {
                    manifest.addMixinConfig(name);
                } else if (lower.endsWith(".accesswidener")) {
                    manifest.accessWidener = name;
                } else if (lower.endsWith(".jar")) {
                    manifest.addNestedJar(name);
                }
            }
            return manifest;
        }

        private void readFabric(ZipFile archive, ZipEntry entry) {
            Object parsed = parseJson(characters(archive, entry));
            if (!(parsed instanceof Map)) {
                report("error", "manifest", "fabric.mod.json is not a JSON object; entrypoints were ignored.");
                return;
            }
            Map root = (Map) parsed;
            modId = jsonText(root, "id");
            modVersion = jsonText(root, "version");
            String declared = jsonText(root, "environment");
            if (declared != null) {
                environment = declared;
            }
            accessWidener = jsonText(root, "accessWidener");
            addEntrypoints(root.get("entrypoints"));
            addMixins(root.get("mixins"));
            addNested(root.get("jars"));
        }

        private void readQuilt(ZipFile archive, ZipEntry entry) {
            Object parsed = parseJson(characters(archive, entry));
            if (!(parsed instanceof Map)) {
                return;
            }
            Object loader = ((Map) parsed).get("quilt_loader");
            if (!(loader instanceof Map)) {
                return;
            }
            Map loaderMap = (Map) loader;
            modId = jsonText(loaderMap, "id");
            modVersion = jsonText(loaderMap, "version");
            addEntrypoints(loaderMap.get("entrypoints"));
            addNested(loaderMap.get("jars"));
            addMixins(((Map) parsed).get("mixin"));
        }

        private void addEntrypoints(Object value) {
            if (!(value instanceof Map)) {
                return;
            }
            for (Object item : ((Map) value).entrySet()) {
                String group = String.valueOf(((Map.Entry) item).getKey());
                Object list = ((Map.Entry) item).getValue();
                if (list instanceof String) {
                    entrypoints.add(new String[] { normaliseGroup(group), (String) list });
                } else if (list instanceof List) {
                    for (Object entryValue : (List) list) {
                        String className = null;
                        if (entryValue instanceof String) {
                            className = (String) entryValue;
                        } else if (entryValue instanceof Map) {
                            className = jsonText((Map) entryValue, "value");
                        }
                        if (className != null && !className.trim().isEmpty()) {
                            entrypoints.add(new String[] { normaliseGroup(group), className.trim() });
                        }
                    }
                }
            }
        }

        private static String normaliseGroup(String group) {
            if ("pre_launch".equals(group) || "prelaunch".equals(group)) {
                return "preLaunch";
            }
            return group;
        }

        private void addMixins(Object value) {
            if (value instanceof String) {
                addMixinConfig((String) value);
                return;
            }
            if (value instanceof Map) {
                addMixinConfig(jsonText((Map) value, "config"));
                return;
            }
            if (value instanceof List) {
                for (Object item : (List) value) {
                    if (item instanceof String) {
                        addMixinConfig((String) item);
                    } else if (item instanceof Map) {
                        addMixinConfig(jsonText((Map) item, "config"));
                    }
                }
            }
        }

         





        private void addMixinConfig(String name) {
            if (name == null || name.trim().length() == 0) {
                return;
            }
            String wanted = configKey(name.trim());
            for (int i = 0; i < mixinConfigs.size(); i++) {
                if (configKey(mixinConfigs.get(i)).equals(wanted)) {
                    return;
                }
            }
            mixinConfigs.add(name.trim());
        }

        private static String configKey(String name) {
            String base = name.replace('\\', '/');
            int slash = base.lastIndexOf('/');
            base = slash < 0 ? base : base.substring(slash + 1);
            base = base.toLowerCase();
            if (base.endsWith(".json")) {
                base = base.substring(0, base.length() - 5);
            }
            if (base.endsWith(".mixins") || base.endsWith(".mixin")) {
                base = base.substring(0, base.lastIndexOf('.'));
            }
            return base;
        }

         
        private void addNestedJar(String name) {
            if (name == null || name.trim().length() == 0) {
                return;
            }
            String wanted = fileKey(name.trim());
            for (int i = 0; i < nestedJars.size(); i++) {
                String stored = nestedJars.get(i);
                if (fileKey(stored).equalsIgnoreCase(wanted)) {
                    if (name.indexOf('/') >= 0 && stored.indexOf('/') < 0) {
                        nestedJars.set(i, name.trim());
                    }
                    return;
                }
            }
            nestedJars.add(name.trim());
        }

        private static String fileKey(String path) {
            String base = path.replace('\\', '/');
            int slash = base.lastIndexOf('/');
            return slash < 0 ? base : base.substring(slash + 1);
        }

        private void addNested(Object value) {
            if (!(value instanceof List)) {
                return;
            }
            for (Object item : (List) value) {
                if (item instanceof String) {
                    addNestedJar((String) item);
                } else if (item instanceof Map) {
                    addNestedJar(jsonText((Map) item, "file"));
                }
            }
        }

         
        private void readModsToml(String text) {
            if (text == null) {
                return;
            }
            boolean inMods = false;
            String pendingId = null;
            String pendingClass = null;
            List<String> lines = new ArrayList<String>();
            String[] raw = text.split("\r?\n");
            for (int i = 0; i < raw.length; i++) {
                lines.add(raw[i]);
            }
            for (int i = 0; i < lines.size(); i++) {
                String trimmed = lines.get(i).trim();
                if (trimmed.startsWith("[[mods]]")) {
                    inMods = true;
                    if (pendingClass != null) {
                        forgeModClasses.add(pendingClass);
                    }
                    pendingId = null;
                    pendingClass = null;
                    continue;
                }
                if (trimmed.startsWith("[[") || trimmed.startsWith("[")) {
                    if (inMods && pendingClass != null) {
                        forgeModClasses.add(pendingClass);
                    }
                    inMods = trimmed.startsWith("[[mods]]");
                    pendingId = null;
                    pendingClass = null;
                    continue;
                }
                if (!inMods || trimmed.startsWith("#")) {
                    continue;
                }
                int equals = trimmed.indexOf('=');
                if (equals < 0) {
                    continue;
                }
                String key = trimmed.substring(0, equals).trim();
                String value = trimmed.substring(equals + 1).trim();
                value = unquote(value);
                if ("modId".equals(key)) {
                    pendingId = value;
                    if (modId == null) {
                        modId = value;
                    }
                } else if ("version".equals(key) && modVersion == null) {
                    modVersion = value;
                } else if ("class".equals(key) || "modClass".equals(key)) {
                    pendingClass = value;
                }
            }
            if (pendingClass != null) {
                forgeModClasses.add(pendingClass);
            }
        }
    }

    private static String unquote(String value) {
        if (value.length() >= 2) {
            char first = value.charAt(0);
            if ((first == '"' || first == '\'') && value.charAt(value.length() - 1) == first) {
                return value.substring(1, value.length() - 1);
            }
        }
        int comment = value.indexOf(" #");
        return comment > 0 ? value.substring(0, comment).trim() : value;
    }

    private static String characters(ZipFile archive, ZipEntry entry) {
        InputStream stream = null;
        try {
            stream = archive.getInputStream(entry);
            return new String(readUtf8(stream), "UTF-8");
        } catch (IOException error) {
            return null;
        } finally {
            close(stream);
        }
    }

    private static byte[] readUtf8(InputStream stream) throws IOException {
        ByteArrayOutputStream buffer = new ByteArrayOutputStream();
        byte[] chunk = new byte[8_192];
        int read;
        while ((read = stream.read(chunk)) > 0) {
            buffer.write(chunk, 0, read);
        }
        return buffer.toByteArray();
    }

    private static String jsonText(Map map, String key) {
        Object value = map.get(key);
        return value instanceof String ? (String) value : null;
    }

     

    private static Object parseJson(String source) {
        if (source == null) {
            return null;
        }
        try {
            JsonReader reader = new JsonReader(source);
            reader.skipSpace();
            Object value = reader.readValue();
            reader.skipSpace();
            return value;
        } catch (RuntimeException error) {
            report("error", "manifest", "Invalid JSON metadata: " + error.getMessage());
            return null;
        }
    }

     
    private static final class JsonReader {
        private final String text;
        private int position;

        JsonReader(String text) {
            this.text = text;
        }

        Object readValue() {
            skipSpace();
            char cursor = peek();
            switch (cursor) {
                case '{':
                    return readObject();
                case '[':
                    return readArray();
                case '"':
                    return readString();
                case 't':
                    expect("true");
                    return Boolean.TRUE;
                case 'f':
                    expect("false");
                    return Boolean.FALSE;
                case 'n':
                    expect("null");
                    return null;
                default:
                    return readNumber();
            }
        }

        private Map readObject() {
            Map result = new java.util.LinkedHashMap();
            consume('{');
            skipSpace();
            if (peek() == '}') {
                position++;
                return result;
            }
            while (true) {
                skipSpace();
                String key = readString();
                skipSpace();
                consume(':');
                result.put(key, readValue());
                skipSpace();
                char next = peek();
                if (next == ',') {
                    position++;
                    continue;
                }
                if (next == '}') {
                    position++;
                    return result;
                }
                throw new RuntimeException("expected ',' or '}' at index " + position);
            }
        }

        private List readArray() {
            List result = new ArrayList();
            consume('[');
            skipSpace();
            if (peek() == ']') {
                position++;
                return result;
            }
            while (true) {
                result.add(readValue());
                skipSpace();
                char next = peek();
                if (next == ',') {
                    position++;
                    continue;
                }
                if (next == ']') {
                    position++;
                    return result;
                }
                throw new RuntimeException("expected ',' or ']' at index " + position);
            }
        }

        private String readString() {
            consume('"');
            StringBuilder builder = new StringBuilder();
            while (true) {
                if (position >= text.length()) {
                    throw new RuntimeException("unterminated string");
                }
                char cursor = text.charAt(position++);
                if (cursor == '"') {
                    return builder.toString();
                }
                if (cursor != '\\') {
                    builder.append(cursor);
                    continue;
                }
                char escaped = text.charAt(position++);
                switch (escaped) {
                    case 'n':
                        builder.append('\n');
                        break;
                    case 't':
                        builder.append('\t');
                        break;
                    case 'r':
                        builder.append('\r');
                        break;
                    case 'b':
                        builder.append('\b');
                        break;
                    case 'f':
                        builder.append('\f');
                        break;
                    case 'u':
                        builder.append((char) Integer.parseInt(text.substring(position, position + 4), 16));
                        position += 4;
                        break;
                    default:
                        builder.append(escaped);
                }
            }
        }

        private Object readNumber() {
            int start = position;
            while (position < text.length() && "+-0123456789.eE".indexOf(text.charAt(position)) >= 0) {
                position++;
            }
            String literal = text.substring(start, position);
            if (literal.isEmpty()) {
                throw new RuntimeException("unexpected character at index " + start);
            }
            try {
                if (literal.indexOf('.') >= 0 || literal.indexOf('e') >= 0 || literal.indexOf('E') >= 0) {
                    return Double.valueOf(literal);
                }
                return Long.valueOf(literal);
            } catch (NumberFormatException error) {
                return literal;
            }
        }

        private void expect(String literal) {
            if (!text.startsWith(literal, position)) {
                throw new RuntimeException("expected " + literal + " at index " + position);
            }
            position += literal.length();
        }

        private void consume(char expected) {
            if (peek() != expected) {
                throw new RuntimeException("expected '" + expected + "' at index " + position);
            }
            position++;
        }

        private char peek() {
            if (position >= text.length()) {
                throw new RuntimeException("unexpected end of JSON");
            }
            return text.charAt(position);
        }

        void skipSpace() {
            while (position < text.length() && Character.isWhitespace(text.charAt(position))) {
                position++;
            }
        }
    }
}
