package org.spongepowered.asm.mixin.transformer;

public final class TestMixinProcessor {
    private int transformedCount = 1;

    boolean canSelect() {
        return transformedCount == 0;
    }
}
