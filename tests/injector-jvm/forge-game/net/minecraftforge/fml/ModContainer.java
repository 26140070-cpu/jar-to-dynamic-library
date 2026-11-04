package net.minecraftforge.fml;

 
public final class ModContainer {
    private final String modId;

    public ModContainer(String modId) {
        this.modId = modId;
    }

    public String getModId() {
        return modId;
    }
}
