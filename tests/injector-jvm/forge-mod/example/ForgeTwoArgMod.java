package example;

import net.minecraftforge.eventbus.api.IEventBus;
import net.minecraftforge.fml.ModContainer;
import net.minecraftforge.fml.common.Mod;

 



@Mod("exampleforge")
public final class ForgeTwoArgMod {
    public ForgeTwoArgMod(IEventBus bus, ModContainer container) {
        bus.register(this);
        ForgeMod.write("forge-container.txt", container == null ? "NULL" : container.getModId());
    }
}
