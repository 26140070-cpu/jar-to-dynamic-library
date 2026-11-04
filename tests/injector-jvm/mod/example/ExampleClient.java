package example;

import java.io.File;
import java.nio.file.Files;

import net.fabricmc.api.ClientModInitializer;

 
public final class ExampleClient implements ClientModInitializer {
    public void onInitializeClient() {
        try {
            File work = new File(System.getProperty("jtdl.test.dir", "."));
            Files.write(new File(work, "fabric-client.txt").toPath(), "client-ran".getBytes("UTF-8"));
        } catch (Exception failure) {
            throw new RuntimeException(failure);
        }
    }
}
