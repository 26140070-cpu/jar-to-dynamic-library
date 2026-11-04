package net.fabricmc.loader.api;

import java.util.Optional;

 




public final class FabricLoader {
    public enum EnvType {
        CLIENT,
        SERVER
    }

    private static final FabricLoader INSTANCE = new FabricLoader();

    private FabricLoader() { }

    public static FabricLoader getInstance() {
        return INSTANCE;
    }

    public EnvType getEnvironmentType() {
        return EnvType.valueOf(System.getProperty("jtdl.test.env", "CLIENT").toUpperCase());
    }

    public Optional<Object> getModContainer(String modId) {
        return Optional.empty();
    }
}
