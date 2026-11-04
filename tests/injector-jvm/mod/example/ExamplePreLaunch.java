package example;

import java.io.File;
import java.nio.file.Files;

import net.fabricmc.loader.api.entrypoint.PreLaunchEntrypoint;

public final class ExamplePreLaunch implements PreLaunchEntrypoint {
    public ExamplePreLaunch() {
        mark("prelaunch-constructor.txt");
    }

    public void onPreLaunch() {
        mark("prelaunch-callback.txt");
    }

    private static void mark(String name) {
        try {
            File work = new File(System.getProperty("jtdl.test.dir", "."));
            Files.write(new File(work, name).toPath(), name.getBytes("UTF-8"));
        } catch (Exception failure) {
            throw new RuntimeException(failure);
        }
    }
}
