package net.minecraftforge.fml;

import java.util.HashMap;
import java.util.Map;
import java.util.Optional;

 




public final class ModList {
    private static final ModList INSTANCE = new ModList();
    private final Map containers = new HashMap();

    private ModList() {
        containers.put("exampleforge", new ModContainer("exampleforge"));
    }

    public static ModList get() {
        return INSTANCE;
    }

    public Optional<ModContainer> getModContainerById(String modId) {
        return Optional.ofNullable((ModContainer) containers.get(modId));
    }
}
