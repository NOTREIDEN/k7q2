package rig;

import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.util.concurrent.LinkedBlockingQueue;

/**
 * A line with a wait on it: what comes in on one port goes on to another
 * port of this machine a fixed time later, and so does every answer. A
 * client that connects through it has that much ping twice over.
 */
final class Delay {
    private Delay() {
    }

    private static final class Piece {
        final byte[] data;
        final long due;

        Piece(byte[] data, long due) {
            this.data = data;
            this.due = due;
        }
    }

    static void start(final int listen, final int target, final int ms) throws Exception {
        final ServerSocket server = new ServerSocket(listen, 8, InetAddress.getLoopbackAddress());
        Thread t = new Thread("delay line") {
            public void run() {
                while (true) {
                    try {
                        Socket a = server.accept();
                        Socket b = new Socket(InetAddress.getLoopbackAddress(), target);
                        a.setTcpNoDelay(true);
                        b.setTcpNoDelay(true);
                        pump(a, b, ms);
                        pump(b, a, ms);
                    } catch (Exception e) {
                        e.printStackTrace();
                    }
                }
            }
        };
        t.setDaemon(true);
        t.start();
    }

    private static void pump(final Socket from, final Socket to, final int ms) {
        final LinkedBlockingQueue<Piece> queue = new LinkedBlockingQueue<Piece>();
        Thread in = new Thread("delay line in") {
            public void run() {
                byte[] buf = new byte[65536];
                try {
                    InputStream s = from.getInputStream();
                    int n;
                    while ((n = s.read(buf)) >= 0) {
                        byte[] d = new byte[n];
                        System.arraycopy(buf, 0, d, 0, n);
                        queue.add(new Piece(d, System.nanoTime() + ms * 1000000L));
                    }
                } catch (Exception e) {
                    // The line is down: the other end is told below.
                }
                queue.add(new Piece(null, 0L));
            }
        };
        Thread out = new Thread("delay line out") {
            public void run() {
                try {
                    OutputStream s = to.getOutputStream();
                    while (true) {
                        Piece p = queue.take();
                        if (p.data == null) break;
                        long left = p.due - System.nanoTime();
                        if (left > 0L) Thread.sleep(left / 1000000L, (int) (left % 1000000L));
                        s.write(p.data);
                        s.flush();
                    }
                } catch (Exception e) {
                    // Nothing more to pass on.
                }
                try {
                    to.close();
                    from.close();
                } catch (Exception e) {
                    // Closed already.
                }
            }
        };
        in.setDaemon(true);
        out.setDaemon(true);
        in.start();
        out.start();
    }
}
