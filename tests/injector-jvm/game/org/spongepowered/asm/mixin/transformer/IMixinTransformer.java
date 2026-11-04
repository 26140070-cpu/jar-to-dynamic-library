package org.spongepowered.asm.mixin.transformer;

public interface IMixinTransformer {
    byte[] transformClassBytes(String name, String transformedName, byte[] classBytes);
}
