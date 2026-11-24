package org.perdura.probe;

import android.app.Activity;
import android.net.wifi.WifiManager;
import android.os.Bundle;
import android.util.Log;
import android.widget.ScrollView;
import android.widget.TextView;

import java.io.BufferedReader;
import java.io.File;
import java.io.InputStreamReader;
import java.net.Inet6Address;
import java.net.InetAddress;
import java.net.NetworkInterface;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;

/** Test bench: runs the embedded perdura binary and shows what it prints. */
public final class MainActivity extends Activity {
    private static final String TAG = "PerduraProbe";

    private TextView output;
    private WifiManager.MulticastLock multicastLock;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);

        output = new TextView(this);
        output.setTextIsSelectable(true);
        output.setPadding(32, 32, 32, 32);
        ScrollView scroll = new ScrollView(this);
        scroll.addView(output);
        setContentView(scroll);

        WifiManager wifi = (WifiManager) getApplicationContext().getSystemService(WIFI_SERVICE);
        multicastLock = wifi.createMulticastLock("perdura-probe");
        multicastLock.setReferenceCounted(false);
        multicastLock.acquire();
        report("multicast-lock " + (multicastLock.isHeld() ? "held" : "NOT-HELD"));

        new Thread(this::runChecks, "perdura-checks").start();
    }

    @Override
    protected void onDestroy() {
        if (multicastLock != null && multicastLock.isHeld()) {
            multicastLock.release();
        }
        super.onDestroy();
    }

    private void runChecks() {
        String binary = getApplicationInfo().nativeLibraryDir + "/libperdura.so";
        report("binary exists=" + new File(binary).exists());

        for (String line : listInterfaces()) {
            report(line);
        }
        for (String line : run(binary)) {
            report("version: " + line);
        }
        File data = new File(getFilesDir(), "perdura");
        for (String line : run(binary, "identity", "--data-dir", data.getAbsolutePath())) {
            report("identity: " + line);
        }
        report("done");
    }

    /** Lists the interfaces that are up and have an IPv6 link-local address. */
    private List<String> listInterfaces() {
        List<String> lines = new ArrayList<>();
        try {
            for (NetworkInterface nif : Collections.list(NetworkInterface.getNetworkInterfaces())) {
                if (!nif.isUp() || nif.isLoopback()) {
                    continue;
                }
                for (InetAddress address : Collections.list(nif.getInetAddresses())) {
                    if (address instanceof Inet6Address && address.isLinkLocalAddress()) {
                        String host = address.getHostAddress();
                        int percent = host.indexOf('%');
                        if (percent >= 0) {
                            host = host.substring(0, percent);
                        }
                        lines.add("interface " + nif.getName()
                                + " index=" + nif.getIndex()
                                + " multicast=" + nif.supportsMulticast()
                                + " address=" + host);
                    }
                }
            }
        } catch (Exception e) {
            lines.add("interface ERROR " + e);
        }
        if (lines.isEmpty()) {
            lines.add("interface NONE");
        }
        return lines;
    }

    /** Runs a command and returns its output lines, followed by its exit code. */
    private List<String> run(String... command) {
        List<String> lines = new ArrayList<>();
        try {
            Process process = new ProcessBuilder(command).redirectErrorStream(true).start();
            try (BufferedReader reader = new BufferedReader(
                    new InputStreamReader(process.getInputStream(), StandardCharsets.UTF_8))) {
                String line;
                while ((line = reader.readLine()) != null) {
                    lines.add(line);
                }
            }
            lines.add("exit=" + process.waitFor());
        } catch (Exception e) {
            lines.add("ERROR " + e);
        }
        return lines;
    }

    private void report(String line) {
        Log.i(TAG, line);
        runOnUiThread(() -> output.append(line + "\n"));
    }
}
