import java.io.File;
import java.io.PrintWriter;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.security.ProtectionDomain;
import java.util.ArrayList;
import java.util.List;

public class ForgeInjector extends Thread {
    private byte[][] classes;

    private ForgeInjector(byte[][] classes) {
        this.classes = classes;
    }

    private static native Class defineClass0(ClassLoader loader, byte[] data, ProtectionDomain protectionDomain);

    private static Object instantiatePublicNoArg(Class type) throws Exception {
        try {
            return type.getConstructor().newInstance();
        } catch (NoSuchMethodException e) {
            throw new Exception("Unsupported entrypoint " + type.getName()
                    + ": a public no-argument constructor is required by this injector.", e);
        } catch (InvocationTargetException e) {
            Throwable cause = e.getCause() == null ? e : e.getCause();
            throw new Exception("Constructor for " + type.getName() + " threw an exception.", cause);
        }
    }

    private static String duplicateClassName(String message) {
        if (message == null) {
            return null;
        }
        int quoted = message.indexOf("duplicate class definition for name: \"");
        if (quoted >= 0) {
            String[] parts = message.split("\"");
            return parts.length > 1 ? parts[1] : null;
        }
        int marker = message.indexOf(" definition for ");
        if (marker < 0 || !message.contains("duplicate")) {
            return null;
        }
        String name = message.substring(marker + " definition for ".length());
        int end = name.indexOf(" (");
        if (end >= 0) {
            name = name.substring(0, end);
        }
        if (name.endsWith(".")) {
            name = name.substring(0, name.length() - 1);
        }
        return name.isEmpty() ? null : name.replace('.', '/');
    }

    public static void inject(byte[][] classes) {
        new Thread(new ForgeInjector(classes)).start();
    }

    private static Class tryGetClass(PrintWriter writer, ClassLoader cl, String... names) throws ClassNotFoundException {
        ClassNotFoundException lastException = null;
        for (String name : names) {
            try {
                return cl.loadClass(name);
            } catch (ClassNotFoundException e) {
                lastException = e;
            }
        }
        throw lastException;
    }

    private static boolean isForgeLikeLoader(String loaderName) {
        return loaderName.contains("LaunchClassLoader")
            || loaderName.contains("RelaunchClassLoader")
            || loaderName.contains("TransformingClassLoader")
            || loaderName.contains("ModuleClassLoader");
    }

    private static boolean isFabricKnotLoader(String loaderName) {
        return loaderName.contains("Knot");
    }

    private List defineAllClasses(PrintWriter writer, ClassLoader cl) throws Exception {
        List defined = new ArrayList();
        for (byte[] classData : classes) {
            try {
                Class tClass;
                try {
                    tClass = defineClass0(cl, classData, null);
                } catch (Throwable e) {
                    if (!(e instanceof LinkageError)) {
                        throw e;
                    }
                    String className = duplicateClassName(e.getMessage());
                    if (className == null) {
                        throw e;
                    }
                    tClass = cl.loadClass(className.replace('/', '.'));
                    writer.println("It is recommended to remove " + className + ".class from your input.jar");
                }
                defined.add(tClass);
            } catch (Exception e) {
                e.printStackTrace(writer);
                throw new Exception("Exception on defineClass", e);
            }
        }
        return defined;
    }

    private void injectForge(PrintWriter writer, ClassLoader cl, List defined) throws Exception {
        Class modAnnotation = tryGetClass(writer, cl,
                "cpw.mods.fml.common.Mod",
                "net.minecraftforge.fml.common.Mod",
                "net.neoforged.fml.common.Mod");
        Class forgeEventHandlerAnnotation = null;
        try {
            forgeEventHandlerAnnotation = tryGetClass(writer, cl,
                    "cpw.mods.fml.common.Mod$EventHandler",
                    "net.minecraftforge.fml.common.Mod$EventHandler");
        } catch (ClassNotFoundException e) {
            writer.println("Legacy @EventHandler annotation not found; using constructor-only initialization");
        }

        ArrayList mods = new ArrayList();
        for (int i = 0; i < defined.size(); i++) {
            Class tClass = (Class) defined.get(i);
            if (tClass.getAnnotation(modAnnotation) == null) {
                continue;
            }
            ArrayList eventHandlers = new ArrayList();
            if (forgeEventHandlerAnnotation != null) {
                Method[] methods = tClass.getDeclaredMethods();
                for (int m = 0; m < methods.length; m++) {
                    Method method = methods[m];
                    if (method.getAnnotation(forgeEventHandlerAnnotation) != null) {
                        eventHandlers.add(method);
                    }
                }
            }
            Object[] mod = new Object[2];
            mod[0] = tClass;
            mod[1] = eventHandlers;
            mods.add(mod);
        }

        if (mods.isEmpty()) {
            throw new Exception("No @Mod classes were found in the embedded JAR. Check the loader version and mod metadata.");
        }

        for (int i = 0; i < mods.size(); i++) {
            Object[] mod = (Object[]) mods.get(i);
            Class modClass = (Class) mod[0];
            ArrayList eventHandlers = (ArrayList) mod[1];
            if (!eventHandlers.isEmpty()) {
                throw new Exception("Unsupported Forge lifecycle for " + modClass.getName()
                        + ": @EventHandler callbacks require real FML event objects. This injector runs after normal mod discovery and will not invoke event handlers with null arguments.");
            }

            writer.println("Instancing " + modClass.getName());
            writer.flush();
            Object modInstance = instantiatePublicNoArg(modClass);
            writer.println("Instanced " + modInstance.getClass().getName());
            writer.flush();
        }
    }

    private void injectFabric(PrintWriter writer, ClassLoader cl, List defined) throws Exception {
        Class modInitializer = tryGetClass(writer, cl, "net.fabricmc.api.ModInitializer");
        Class clientInitializer = null;
        Class serverInitializer = null;
        try {
            clientInitializer = cl.loadClass("net.fabricmc.api.ClientModInitializer");
        } catch (ClassNotFoundException e) {
            writer.println("ClientModInitializer not found");
        }
        try {
            serverInitializer = cl.loadClass("net.fabricmc.api.DedicatedServerModInitializer");
        } catch (ClassNotFoundException e) {
            writer.println("DedicatedServerModInitializer not found");
        }

        boolean isClient = false;
        boolean isServer = false;
        try {
            Class fabricLoader = cl.loadClass("net.fabricmc.loader.api.FabricLoader");
            Object instance = fabricLoader.getMethod("getInstance").invoke(null);
            Object env = instance.getClass().getMethod("getEnvironmentType").invoke(instance);
            String envName = String.valueOf(env);
            isClient = "CLIENT".equals(envName);
            isServer = "SERVER".equals(envName);
            writer.println("Fabric EnvType=" + envName);
        } catch (Throwable e) {
            writer.println("Could not detect Fabric EnvType (" + e + "); will try all side entrypoints");
            isClient = true;
            isServer = true;
        }
        writer.flush();

        Method onInitialize = modInitializer.getMethod("onInitialize");
        Method onInitializeClient = clientInitializer != null ? clientInitializer.getMethod("onInitializeClient") : null;
        Method onInitializeServer = serverInitializer != null ? serverInitializer.getMethod("onInitializeServer") : null;

        int mainCount = 0;
        int clientCount = 0;
        int serverCount = 0;

        for (int i = 0; i < defined.size(); i++) {
            Class tClass = (Class) defined.get(i);
            if (tClass.isInterface() || java.lang.reflect.Modifier.isAbstract(tClass.getModifiers())) {
                continue;
            }

            if (modInitializer.isAssignableFrom(tClass)) {
                Object inst = instantiatePublicNoArg(tClass);
                writer.println("Fabric main entrypoint: " + tClass.getName());
                writer.flush();
                onInitialize.invoke(inst);
                mainCount++;
            }
            if (isClient && clientInitializer != null && clientInitializer.isAssignableFrom(tClass)) {
                Object inst = instantiatePublicNoArg(tClass);
                writer.println("Fabric client entrypoint: " + tClass.getName());
                writer.flush();
                onInitializeClient.invoke(inst);
                clientCount++;
            }
            if (isServer && serverInitializer != null && serverInitializer.isAssignableFrom(tClass)) {
                Object inst = instantiatePublicNoArg(tClass);
                writer.println("Fabric server entrypoint: " + tClass.getName());
                writer.flush();
                onInitializeServer.invoke(inst);
                serverCount++;
            }
        }

        writer.println("Fabric entrypoints invoked: main=" + mainCount + " client=" + clientCount + " server=" + serverCount);
        writer.flush();
        if (mainCount == 0 && clientCount == 0 && serverCount == 0) {
            writer.println("WARNING: no Fabric entrypoint classes found among defined classes");
            writer.println("  (classes must implement ModInitializer / ClientModInitializer / DedicatedServerModInitializer");
            writer.println("   and have a public no-arg constructor; fabric.mod.json is not consulted)");
            writer.flush();
            throw new Exception("No compatible Fabric entrypoint class was found in the embedded JAR.");
        }
    }

    @Override
    public void run() {
        try (PrintWriter writer = new PrintWriter(System.getProperty("user.home") + File.separator + "jar-to-dynamic-library-log.txt", "UTF-8")) {
            writer.println("Starting!");
            writer.flush();
            try {
                ClassLoader cl = null;
                boolean fabric = false;
                boolean forgeLike = false;
                for (Thread thread : Thread.getAllStackTraces().keySet()) {
                    ClassLoader threadLoader;
                    if (thread == null || thread.getContextClassLoader() == null
                            || (threadLoader = thread.getContextClassLoader()).getClass() == null
                            || threadLoader.getClass().getName() == null) {
                        continue;
                    }
                    String loaderName = threadLoader.getClass().getName();
                    writer.println("Thread: " + thread.getName() + " [" + loaderName + "]");
                    writer.flush();
                    if (isFabricKnotLoader(loaderName)) {
                        cl = threadLoader;
                        fabric = true;
                        break;
                    }
                    if (isForgeLikeLoader(loaderName)) {
                        cl = threadLoader;
                        forgeLike = true;
                        break;
                    }
                }
                if (cl == null) {
                    throw new Exception("ClassLoader is null (no Knot / LaunchClassLoader / TransformingClassLoader / ModuleClassLoader found)");
                }
                this.setContextClassLoader(cl);

                if (!fabric && forgeLike) {
                    try {
                        tryGetClass(writer, cl,
                                "cpw.mods.fml.common.Mod",
                                "net.minecraftforge.fml.common.Mod",
                                "net.neoforged.fml.common.Mod");
                    } catch (ClassNotFoundException e) {
                        try {
                            cl.loadClass("net.fabricmc.api.ModInitializer");
                            fabric = true;
                            forgeLike = false;
                            writer.println("No @Mod annotation class; treating as Fabric");
                        } catch (ClassNotFoundException e2) {
                            throw new Exception("Neither Forge/NeoForge @Mod nor Fabric ModInitializer found", e);
                        }
                    }
                }

                writer.println("Loading " + classes.length + " classes (mode=" + (fabric ? "Fabric/Knot" : "Forge/NeoForge") + ")");
                writer.flush();
                List defined = defineAllClasses(writer, cl);
                writer.println(classes.length + " loaded successfully");
                writer.flush();

                if (fabric) {
                    injectFabric(writer, cl, defined);
                } else {
                    injectForge(writer, cl, defined);
                }

                writer.println("Successfully injected");
                writer.flush();
            } catch (Throwable e) {
                e.printStackTrace(writer);
                writer.flush();
            }
        } catch (Throwable e) {
            e.printStackTrace();
        }
    }
}
