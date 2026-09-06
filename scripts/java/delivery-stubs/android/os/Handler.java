package android.os;
import java.util.ArrayDeque;
/** Test-only deterministic scheduling boundary, NOT an ART/Looper implementation. */
public class Handler {
    private final ArrayDeque<Runnable> queue = new ArrayDeque<>();
    private boolean accepting = true;
    private int accepted, rejected;
    public synchronized boolean post(Runnable runnable) {
        if (!accepting) { rejected++; return false; }
        queue.add(runnable); accepted++; return true;
    }
    public boolean sendEmptyMessage(int what) {
        Message message = new Message(); message.what = what;
        return post(() -> handleMessage(message));
    }
    public void handleMessage(Message message) {}
    public synchronized void removeCallbacksAndMessages(Object token) { queue.clear(); }
    public synchronized void setAccepting(boolean value) { accepting = value; }
    public synchronized int acceptedCount() { return accepted; }
    public synchronized int rejectedCount() { return rejected; }
    public synchronized int queuedCount() { return queue.size(); }
    public synchronized Runnable takeNext() { return queue.poll(); }
    public void drain() {
        for (;;) {
            Runnable next;
            synchronized (this) { next = queue.poll(); }
            if (next == null) return;
            next.run();
        }
    }
}
