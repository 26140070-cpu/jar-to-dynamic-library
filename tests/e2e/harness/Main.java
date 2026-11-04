import java.io.File;
import java.nio.file.Files;
import java.nio.file.Paths;

public class Main {
    public static void main(String[] args) throws Exception {
        FakeLaunchClassLoader loader = new FakeLaunchClassLoader(Main.class.getClassLoader());

         
         
        Class<?> gameMarker = loader.define(Files.readAllBytes(Paths.get(args[1], "testmod", "Marker.class")));

        Thread fakeForge = new Thread(() -> {
            try { Thread.sleep(10_000); } catch (InterruptedException ignored) { }
        }, "fake-forge-thread");
        fakeForge.setDaemon(true);
        fakeForge.setContextClassLoader(loader);
        fakeForge.start();

        System.load(args[0]);

        File marker = new File(System.getProperty("user.home"), "mod-ran.txt");
        for (int i = 0; i < 50 && !marker.exists(); i++) {
            Thread.sleep(100);
        }
        if (!marker.exists()) {
            System.out.println("FAIL: mod did not run");
            System.exit(1);
        }
        String content = new String(Files.readAllBytes(marker.toPath()), "UTF-8").trim();
        System.out.println("marker: " + content);
        String expected = "hello-from-base|tag-from-marker|" + System.identityHashCode(gameMarker);
        if (!content.equals(expected)) {
            System.out.println("FAIL: the mod did not reuse the class the game loader already had");
            System.exit(2);
        }

        File log = new File(System.getProperty("user.home"), "jar-to-dynamic-library-log.txt");
        String logText = new String(Files.readAllBytes(log.toPath()), "UTF-8");
        if (!logText.contains("Game class loader: FakeLaunchClassLoader")) {
            System.out.println("FAIL: the injector did not detect the game class loader");
            System.exit(3);
        }
        if (logText.contains("JTDL|error|")) {
            System.out.println("FAIL: the injector reported an error");
            System.exit(4);
        }
        System.out.println("the mod linked against the game's own Marker class");
        System.exit(0);
    }
}
