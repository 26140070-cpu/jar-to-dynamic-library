package example;

import java.io.File;
import java.nio.file.Files;

import net.minecraftforge.eventbus.api.IEventBus;
import net.minecraftforge.fml.common.Mod;

 



@Mod("exampleforge")
public final class ForgeMod {
    public ForgeMod(IEventBus bus) {
        bus.register(this);
        write("forge-bus.txt", "ctor-bus");
    }

    static void write(String name, String text) {
        try {
            File work = new File(System.getProperty("jtdl.test.dir", "."));
            Files.write(new File(work, name).toPath(), text.getBytes("UTF-8"));
        } catch (Exception failure) {
            throw new RuntimeException(failure);
        }
    }
}
