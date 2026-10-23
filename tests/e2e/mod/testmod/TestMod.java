package testmod;

import java.nio.file.Files;
import java.nio.file.Paths;
import net.minecraftforge.fml.common.Mod;

@Mod(modid = "testmod")
public class TestMod extends TestBase implements Marker {
    public TestMod() throws Exception {
        String home = System.getProperty("user.home");
        Files.write(Paths.get(home, "mod-ran.txt"), (hello() + "|" + tag()).getBytes("UTF-8"));
    }

    @Override
    public String tag() {
        return "tag-from-marker";
    }
}
