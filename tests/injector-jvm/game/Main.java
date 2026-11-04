import java.io.File;
import java.lang.management.ManagementFactory;
import java.nio.file.Files;

 




public final class Main {
    public static void main(String[] args) throws Exception {
        File work = new File(args[0]);
        KnotClassLoader knot = new KnotClassLoader(Main.class.getClassLoader());
        Class target = Class.forName("game.Target", true, knot);
        Files.write(new File(work, "mixin-before.txt").toPath(),
                target.getMethod("value").invoke(null).toString().getBytes("UTF-8"));

        Thread client = new Thread(new Runnable() {
            public void run() {
                try {
                    Thread.sleep(180_000L);
                } catch (InterruptedException ignored) {
                     
                }
            }
        }, "Render thread");
        client.setDaemon(true);
        client.setContextClassLoader(knot);
        client.start();

        String name = ManagementFactory.getRuntimeMXBean().getName();
        File pidFile = new File(work, "game.pid");
        Files.write(pidFile.toPath(), name.split("@")[0].getBytes("UTF-8"));

        File stop = new File(work, "game-stop");
        long deadline = System.currentTimeMillis() + 180_000L;
        File check = new File(work, "mixin-check");
        while (!stop.exists() && System.currentTimeMillis() < deadline) {
            if (check.exists()) {
                Files.write(new File(work, "mixin-after.txt").toPath(),
                        target.getMethod("value").invoke(null).toString().getBytes("UTF-8"));
                check.delete();
            }
            Thread.sleep(50L);
        }
        System.out.println("game exiting");
    }
}
