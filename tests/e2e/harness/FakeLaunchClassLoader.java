import java.net.URL;
import java.net.URLClassLoader;

public class FakeLaunchClassLoader extends URLClassLoader {
    public FakeLaunchClassLoader(ClassLoader parent) {
        super(new URL[0], parent);
    }

    public Class<?> define(byte[] bytes) {
        return defineClass(null, bytes, 0, bytes.length);
    }
}
