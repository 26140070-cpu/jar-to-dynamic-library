package example;

import java.io.File;
import java.nio.file.Files;

public final class ExampleMeteorAddon extends MeteorAddon {
    public void onInitialize() {
        try {
            File work = new File(System.getProperty("jtdl.test.dir", "."));
            Files.write(new File(work, "meteor-entrypoint.txt").toPath(), "meteor-ran".getBytes("UTF-8"));
        } catch (Exception failure) {
            throw new RuntimeException("the custom entrypoint could not record its evidence", failure);
        }
    }
}
