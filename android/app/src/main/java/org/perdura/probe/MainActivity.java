package org.perdura.probe;

import android.app.Activity;
import android.net.wifi.WifiManager;
import android.os.Bundle;
import android.util.Log;
import android.view.View;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.io.BufferedReader;
import java.io.File;
import java.io.IOException;
import java.io.InputStreamReader;
import java.net.Inet6Address;
import java.net.InetAddress;
import java.net.NetworkInterface;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;

/**
 * Test bench: runs the embedded perdura binary and shows what it prints.
 *
 * Intent extras for automated runs:
 *   autostart (boolean): start discovery and listening when the app opens.
 *   send (string): send this message to the first peer discovered.
 */
public final class MainActivity extends Activity {
    private static final String TAG = "PerduraProbe";
    private static final String[] PREFERRED_PREFIXES = {"wlan", "swlan", "ap", "softap", "wl"};

    private interface LineHandler {
        void onLine(String line);
    }

    private static final class Candidate {
        final String name;
        final int index;
        final String address;
        final int rank;

        Candidate(String name, int index, String address, int rank) {
            this.name = name;
            this.index = index;
            this.address = address;
            this.rank = rank;
        }
    }

    private final List<Candidate> candidates = new ArrayList<>();
    private final Map<String, String> peers = new ConcurrentHashMap<>();
    private int selected;

    private TextView status;
    private TextView output;
    private ScrollView scroll;
    private EditText message;
    private WifiManager.MulticastLock multicastLock;
    private String binary;
    private File dataDir;

    private volatile boolean running;
    private volatile Process discoverProcess;
    private volatile Process listenProcess;
    private volatile String fingerprint = "?";
    private volatile String pendingAutoSend;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);

        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setPadding(24, 24, 24, 24);

        status = new TextView(this);
        status.setTextIsSelectable(true);
        root.addView(status);

        root.addView(button("Atualizar interfaces", this::onRefreshInterfaces));
        root.addView(button("Proxima interface", this::onNextInterface));
        root.addView(button("Iniciar", this::start));
        root.addView(button("Parar", this::stop));

        message = new EditText(this);
        message.setHint("mensagem (sem acentos)");
        message.setSingleLine(true);
        root.addView(message);
        root.addView(button("Enviar para o par", this::onSend));

        output = new TextView(this);
        output.setTextIsSelectable(true);
        scroll = new ScrollView(this);
        scroll.addView(output);
        root.addView(scroll, new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT, 0, 1f));

        setContentView(root);

        WifiManager wifi = (WifiManager) getApplicationContext().getSystemService(WIFI_SERVICE);
        multicastLock = wifi.createMulticastLock("perdura-probe");
        multicastLock.setReferenceCounted(false);
        multicastLock.acquire();
        report("multicast-lock " + (multicastLock.isHeld() ? "held" : "NOT-HELD"));

        binary = getApplicationInfo().nativeLibraryDir + "/libperdura.so";
        dataDir = new File(getFilesDir(), "perdura");
        report("binary exists=" + new File(binary).exists());

        refreshInterfaces();
        updateStatus();

        final boolean autostart = getIntent().getBooleanExtra("autostart", false);
        pendingAutoSend = getIntent().getStringExtra("send");

        new Thread(() -> {
            loadIdentity();
            if (autostart) {
                runOnUiThread(this::start);
            }
        }, "perdura-identity").start();
    }

    @Override
    protected void onDestroy() {
        stop();
        if (multicastLock != null && multicastLock.isHeld()) {
            multicastLock.release();
        }
        super.onDestroy();
    }

    private Button button(String label, Runnable action) {
        Button button = new Button(this);
        button.setText(label);
        button.setAllCaps(false);
        button.setOnClickListener(view -> action.run());
        return button;
    }

    private void onRefreshInterfaces() {
        if (running) {
            report("interfaces: stop before refreshing");
            return;
        }
        refreshInterfaces();
        updateStatus();
    }

    private void onNextInterface() {
        if (running) {
            report("interfaces: stop before switching");
            return;
        }
        if (candidates.size() > 1) {
            selected = (selected + 1) % candidates.size();
        }
        updateStatus();
    }

    private void onSend() {
        String text = message.getText().toString().trim();
        send(text.isEmpty() ? "ola" : text);
    }

    /** Collects the interfaces that are up, multicast-capable and have an IPv6 link-local address. */
    private void refreshInterfaces() {
        candidates.clear();
        selected = 0;
        try {
            for (NetworkInterface nif : Collections.list(NetworkInterface.getNetworkInterfaces())) {
                String name = nif.getName();
                if (!nif.isUp() || nif.isLoopback() || !nif.supportsMulticast()
                        || name.startsWith("dummy")) {
                    continue;
                }
                for (InetAddress address : Collections.list(nif.getInetAddresses())) {
                    if (address instanceof Inet6Address && address.isLinkLocalAddress()) {
                        String host = address.getHostAddress();
                        int percent = host.indexOf('%');
                        if (percent >= 0) {
                            host = host.substring(0, percent);
                        }
                        candidates.add(new Candidate(name, nif.getIndex(), host, rank(name)));
                        break;
                    }
                }
            }
        } catch (Exception e) {
            report("interface ERROR " + e);
        }
        Collections.sort(candidates, (a, b) ->
                a.rank != b.rank ? Integer.compare(a.rank, b.rank) : a.name.compareTo(b.name));
        for (Candidate candidate : candidates) {
            report("interface " + candidate.name
                    + " index=" + candidate.index
                    + " address=" + candidate.address);
        }
        if (candidates.isEmpty()) {
            report("interface NONE");
        }
    }

    private static int rank(String name) {
        for (int i = 0; i < PREFERRED_PREFIXES.length; i++) {
            if (name.startsWith(PREFERRED_PREFIXES[i])) {
                return i;
            }
        }
        return 100;
    }

    private void loadIdentity() {
        try {
            Process process = launch(binary, "identity", "--data-dir", dataDir.getAbsolutePath());
            pump(process, "identity: ", line -> {
                if (line.startsWith("identity ")) {
                    fingerprint = line.substring("identity ".length());
                }
            }, false);
            report("identity: exit=" + process.waitFor());
        } catch (Exception e) {
            report("identity: ERROR " + e);
        }
        runOnUiThread(this::updateStatus);
    }

    private void start() {
        if (running) {
            return;
        }
        if (candidates.isEmpty()) {
            report("start: no interface with an IPv6 link-local address");
            return;
        }
        final Candidate candidate = candidates.get(selected);
        running = true;
        report("start: interface " + candidate.name + " index=" + candidate.index);
        new Thread(() -> discoverLoop(candidate), "perdura-discover").start();
        new Thread(this::listenLoop, "perdura-listen").start();
        updateStatus();
    }

    private void stop() {
        if (!running) {
            return;
        }
        running = false;
        Process discover = discoverProcess;
        if (discover != null) {
            discover.destroy();
        }
        Process listen = listenProcess;
        if (listen != null) {
            listen.destroy();
        }
        discoverProcess = null;
        listenProcess = null;
        peers.clear();
        report("stopped");
        updateStatus();
    }

    private void discoverLoop(Candidate candidate) {
        try {
            Process process = launch(binary, "discover", "--ifindex", String.valueOf(candidate.index));
            discoverProcess = process;
            if (!running) {
                process.destroy();
            }
            pump(process, "discover: ", this::onDiscoverLine, true);
            int code = process.waitFor();
            if (running) {
                report("discover: exit=" + code);
            }
        } catch (Exception e) {
            report("discover: ERROR " + e);
        }
    }

    private void onDiscoverLine(String line) {
        String[] parts = line.split(" ");
        if (parts.length >= 3 && parts[0].equals("found")) {
            peers.put(parts[1], parts[2]);
            runOnUiThread(this::updateStatus);
            String text = pendingAutoSend;
            if (text != null) {
                pendingAutoSend = null;
                send(text);
            }
        } else if (parts.length >= 2 && parts[0].equals("lost")) {
            peers.remove(parts[1]);
            runOnUiThread(this::updateStatus);
        }
    }

    /** Accepts one channel at a time, for as long as the bench is running. */
    private void listenLoop() {
        while (running) {
            try {
                long started = System.currentTimeMillis();
                Process process = launch(binary, "listen", "--data-dir", dataDir.getAbsolutePath());
                listenProcess = process;
                if (!running) {
                    process.destroy();
                }
                pump(process, "listen: ", null, true);
                process.waitFor();
                if (!running) {
                    return;
                }
                boolean failedFast = System.currentTimeMillis() - started < 1000;
                Thread.sleep(failedFast ? 2000 : 200);
            } catch (InterruptedException e) {
                return;
            } catch (Exception e) {
                report("listen: ERROR " + e);
                return;
            }
        }
    }

    private void send(String text) {
        final String address = firstPeer();
        if (address == null) {
            report("send: no peer discovered yet");
            return;
        }
        report("send: to " + address);
        new Thread(() -> {
            try {
                Process process = launch(binary, "send",
                        "--data-dir", dataDir.getAbsolutePath(),
                        "--to", address,
                        "--message", text);
                pump(process, "send: ", null, false);
                report("send: exit=" + process.waitFor());
            } catch (Exception e) {
                report("send: ERROR " + e);
            }
        }, "perdura-send").start();
    }

    private String firstPeer() {
        for (String address : peers.values()) {
            return address;
        }
        return null;
    }

    private Process launch(String... command) throws IOException {
        return new ProcessBuilder(command).redirectErrorStream(true).start();
    }

    /** Reports every output line of a process and passes it to the handler. */
    private void pump(Process process, String prefix, LineHandler handler, boolean quietWhenStopped) {
        try (BufferedReader reader = new BufferedReader(
                new InputStreamReader(process.getInputStream(), StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                report(prefix + line);
                if (handler != null) {
                    handler.onLine(line);
                }
            }
        } catch (IOException e) {
            if (!(quietWhenStopped && !running)) {
                report(prefix + "ERROR " + e);
            }
        }
    }

    private void updateStatus() {
        StringBuilder text = new StringBuilder();
        text.append("identidade: ").append(fingerprint).append('\n');
        if (candidates.isEmpty()) {
            text.append("interface: nenhuma com IPv6 local\n");
        } else {
            Candidate candidate = candidates.get(selected);
            text.append("interface: ").append(candidate.name)
                    .append(" (").append(selected + 1).append('/').append(candidates.size())
                    .append(") indice ").append(candidate.index).append('\n');
            text.append("endereco: ").append(candidate.address).append('\n');
        }
        text.append("pares: ").append(peers.size());
        String address = firstPeer();
        if (address != null) {
            text.append(" -> ").append(address);
        }
        text.append('\n');
        text.append("estado: ").append(running ? "rodando" : "parado");
        status.setText(text);
    }

    private void report(String line) {
        Log.i(TAG, line);
        runOnUiThread(() -> {
            output.append(line + "\n");
            scroll.post(() -> scroll.fullScroll(View.FOCUS_DOWN));
        });
    }
}
