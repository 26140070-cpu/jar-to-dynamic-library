import java.net.URL;
import java.net.URLClassLoader;

 





public final class KnotClassLoader extends URLClassLoader {
    public KnotClassLoader(ClassLoader parent) {
        super(new URL[0], parent);
    }

    @Override
    public void addURL(URL url) {
        super.addURL(url);
    }
}
