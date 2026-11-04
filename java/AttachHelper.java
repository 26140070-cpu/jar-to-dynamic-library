import com.sun.tools.attach.VirtualMachine;
import com.sun.tools.attach.VirtualMachineDescriptor;
import com.sun.tools.attach.AttachOperationFailedException;

import java.util.List;

 























public final class AttachHelper {
    private AttachHelper() { }

    public static void main(String[] args) {
        try {
            if (args.length == 0) {
                fail("usage: AttachHelper <list|inject|inject-jar> ...");
            }
            switch (args[0]) {
                case "list":
                    list();
                    break;
                case "inject":
                    if (args.length < 3) {
                        fail("usage: AttachHelper inject <pid> <nativeLibraryPath> [options]");
                    }
                    injectNative(args[1], args[2], args.length > 3 ? args[3] : "");
                    break;
                case "inject-jar":
                    if (args.length < 3) {
                        fail("usage: AttachHelper inject-jar <pid> <agentJarPath> [options]");
                    }
                    injectJar(args[1], args[2], args.length > 3 ? args[3] : "");
                    break;
                default:
                    fail("unknown command: " + args[0]);
            }
        } catch (Throwable error) {
            System.out.println("ERROR " + describe(error));
            System.exit(1);
        }
    }

    private static String describe(Throwable error) {
        String message = String.valueOf(error);
        if (error instanceof AttachOperationFailedException
                && message.contains("Unable to access the filesystem of the target process")) {
            return message + "\n"
                    + "On Linux, the JDK could not access the target JVM's temporary directory. "
                    + "The app and game may be in different containers, Flatpak/sandbox environments, "
                    + "mount namespaces, or root filesystems. Run both outside the sandbox or inside "
                    + "the same container with a shared writable /tmp, then restart the game and retry. "
                    + "Changing JTDL's permissions or switching between the Java-agent and native routes "
                    + "will not fix an isolated target filesystem.";
        }
        return message;
    }

    private static void list() throws Exception {
        List<VirtualMachineDescriptor> vms = VirtualMachine.list();
        for (VirtualMachineDescriptor descriptor : vms) {
            String display = descriptor.displayName() == null ? "" : descriptor.displayName();
            display = display.replace('\t', ' ').replace('\n', ' ').replace('\r', ' ');
            System.out.println(descriptor.id() + "\t" + display);
        }
    }

    private static void injectNative(String pid, String libraryPath, String options) throws Exception {
        VirtualMachine vm = VirtualMachine.attach(pid);
        try {
            vm.loadAgentPath(libraryPath, options.isEmpty() ? null : options);
            System.out.println("OK");
        } finally {
            vm.detach();
        }
    }

    private static void injectJar(String pid, String jarPath, String options) throws Exception {
        VirtualMachine vm = VirtualMachine.attach(pid);
        try {
            vm.loadAgent(jarPath, options.isEmpty() ? null : options);
            System.out.println("OK");
        } finally {
            vm.detach();
        }
    }

    private static void fail(String message) {
        System.out.println("ERROR " + message);
        System.exit(2);
    }
}
