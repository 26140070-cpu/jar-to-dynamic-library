package org.spongepowered.asm.mixin;

import org.spongepowered.asm.mixin.transformer.TestMixinTransformer;

public final class Mixins {
    private static volatile boolean unvisited;

    public static void addConfiguration(String config) {
        if ("examplemod.mixins.json".equals(config)) {
            unvisited = true;
            TestMixinTransformer.enable();
        }
    }

    public static int getUnvisitedCount() {
        return unvisited ? 1 : 0;
    }

    public static void selectLateConfigurations() {
        unvisited = false;
    }
}
