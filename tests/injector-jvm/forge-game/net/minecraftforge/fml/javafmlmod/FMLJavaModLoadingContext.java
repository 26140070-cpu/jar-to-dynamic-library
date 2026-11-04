package net.minecraftforge.fml.javafmlmod;

import java.util.ArrayList;
import java.util.List;

import net.minecraftforge.eventbus.api.IEventBus;

 




public final class FMLJavaModLoadingContext {
    private static final FMLJavaModLoadingContext INSTANCE = new FMLJavaModLoadingContext();
    private final List registered = new ArrayList();

    private FMLJavaModLoadingContext() { }

    public static FMLJavaModLoadingContext get() {
        return INSTANCE;
    }

    public IEventBus getModEventBus() {
        return new IEventBus() {
            public void register(Object target) {
                registered.add(target);
            }
        };
    }

    public List registered() {
        return registered;
    }
}
