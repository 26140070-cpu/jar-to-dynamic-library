package example;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.InputStream;
import java.nio.file.Files;

import net.fabricmc.api.ModInitializer;







public final class ExampleMod implements ModInitializer {
    public void onInitialize() {
        try {
            File work = new File(System.getProperty("jtdl.test.dir", "."));
            Files.write(new File(work, "fabric-main.txt").toPath(), "main-ran".getBytes("UTF-8"));

            InputStream resource = getClass().getClassLoader()
                    .getResourceAsStream("assets/examplemod/lang/es_es.json");
            String text = resource == null ? "MISSING" : new String(readAll(resource), "UTF-8");
            Files.write(new File(work, "resource.txt").toPath(), text.getBytes("UTF-8"));

            String nested;
            try {
                Class<?> helper = Class.forName("nested.NestedHelper");
                nested = String.valueOf(helper.getMethod("greeting").invoke(null));
            } catch (Throwable error) {
                nested = "FAILED " + error;
            }
            Files.write(new File(work, "nested.txt").toPath(), nested.getBytes("UTF-8"));

            String loader = getClass().getClassLoader().getClass().getName();
            Files.write(new File(work, "loader.txt").toPath(), loader.getBytes("UTF-8"));
        } catch (Exception failure) {
            throw new RuntimeException("the test mod could not record its evidence", failure);
        }
    }

    private static byte[] readAll(InputStream stream) throws java.io.IOException {
        ByteArrayOutputStream buffer = new ByteArrayOutputStream();
        byte[] chunk = new byte[4096];
        int read;
        while ((read = stream.read(chunk)) > 0) {
            buffer.write(chunk, 0, read);
        }
        stream.close();
        return buffer.toByteArray();
    }
}
