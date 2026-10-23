import java.io.File;
import java.nio.file.Files;
import java.nio.file.Paths;

public class Main {
    public static void main(String[] args) throws Exception {
        FakeLaunchClassLoader loader = new FakeLaunchClassLoader(Main.class.getClassLoader());

        loader.define(Files.readAllBytes(Paths.get(args[1], "testmod", "Marker.class")));
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
        if (!content.equals("hello-from-base|tag-from-marker")) {
            System.exit(2);
        }

        File log = new File(System.getProperty("user.home"), "jar-to-dynamic-library-log.txt");
        String logText = new String(Files.readAllBytes(log.toPath()), "UTF-8");
        if (!logText.contains("It is recommended to remove testmod/Marker.class")) {
            System.out.println("FAIL: duplicate class was not handled");
            System.exit(3);
        }
        System.out.println("duplicate class handled");
        System.exit(0);
    }
}
