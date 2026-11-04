import java.net.URL;
import java.net.URLClassLoader;

 




public final class LaunchClassLoader extends URLClassLoader {
    public LaunchClassLoader(ClassLoader parent) {
        super(new URL[0], parent);
    }

    @Override
    public void addURL(URL url) {
        super.addURL(url);
    }
}
