import java.io.File;
import java.lang.management.ManagementFactory;
import java.nio.file.Files;

import net.minecraftforge.fml.javafmlmod.FMLJavaModLoadingContext;

 




public final class ForgeMain {
    public static void main(String[] args) throws Exception {
        File work = new File(args[0]);
        LaunchClassLoader launch = new LaunchClassLoader(ForgeMain.class.getClassLoader());

        Thread client = new Thread(new Runnable() {
            public void run() {
                try {
                    Thread.sleep(180_000L);
                } catch (InterruptedException ignored) {
                     
                }
            }
        }, "Render thread");
        client.setDaemon(true);
        client.setContextClassLoader(launch);
        client.start();

        String name = ManagementFactory.getRuntimeMXBean().getName();
        Files.write(new File(work, "game.pid").toPath(), name.split("@")[0].getBytes("UTF-8"));
        Files.write(new File(work, "bus.txt")
                .toPath(), String.valueOf(FMLJavaModLoadingContext.get().registered()).getBytes("UTF-8"));

        File stop = new File(work, "game-stop");
        long deadline = System.currentTimeMillis() + 180_000L;
        while (!stop.exists() && System.currentTimeMillis() < deadline) {
            Thread.sleep(50L);
        }
         
        Files.write(new File(work, "bus.txt")
                .toPath(), String.valueOf(FMLJavaModLoadingContext.get().registered()).getBytes("UTF-8"));
        System.out.println("game exiting");
    }
}
