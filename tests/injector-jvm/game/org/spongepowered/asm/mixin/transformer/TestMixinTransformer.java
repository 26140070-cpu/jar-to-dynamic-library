package org.spongepowered.asm.mixin.transformer;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;

public final class TestMixinTransformer implements IMixinTransformer {
    private static volatile boolean enabled;
    private final TestMixinProcessor processor = new TestMixinProcessor();

    public static void enable() {
        enabled = true;
    }

    public byte[] transformClassBytes(String name, String transformedName, byte[] classBytes) {
        if (!enabled || !"game.Target".equals(transformedName) || !processor.canSelect()) {
            return null;
        }
        org.spongepowered.asm.mixin.Mixins.selectLateConfigurations();
        try {
            InputStream input = getClass().getClassLoader()
                    .getResourceAsStream("jtdl/transformed/game/Target.class");
            if (input == null) {
                throw new IllegalStateException("missing transformed fixture class");
            }
            ByteArrayOutputStream output = new ByteArrayOutputStream();
            byte[] buffer = new byte[4096];
            int count;
            while ((count = input.read(buffer)) != -1) {
                output.write(buffer, 0, count);
            }
            input.close();
            return output.toByteArray();
        } catch (Exception failure) {
            throw new IllegalStateException(failure);
        }
    }
}
