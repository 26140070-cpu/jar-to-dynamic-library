package example;

import java.io.File;
import java.nio.file.Files;

import net.fabricmc.api.DedicatedServerModInitializer;


public final class ExampleServer implements DedicatedServerModInitializer {
    public void onInitializeServer() {
        try {
            File work = new File(System.getProperty("jtdl.test.dir", "."));
            Files.write(new File(work, "fabric-server.txt").toPath(), "server-ran".getBytes("UTF-8"));
        } catch (Exception failure) {
            throw new RuntimeException(failure);
        }
    }
}
