package org.spongepowered.asm.mixin;

import org.spongepowered.asm.mixin.transformer.IMixinTransformer;

public final class MixinEnvironment {
    private static final MixinEnvironment INSTANCE = new MixinEnvironment();
    private final IMixinTransformer transformer = new org.spongepowered.asm.mixin.transformer.TestMixinTransformer();

    public static MixinEnvironment getCurrentEnvironment() {
        return INSTANCE;
    }

    public Object getActiveTransformer() {
        return transformer;
    }
}
