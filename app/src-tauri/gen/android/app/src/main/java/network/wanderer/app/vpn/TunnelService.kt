package network.wanderer.app.vpn

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import android.util.Log
import androidx.core.app.NotificationCompat
import network.wanderer.app.MainActivity
import network.wanderer.app.R
import network.wanderer.app.xray.Protector
import network.wanderer.app.xray.Xray
import java.util.concurrent.Callable
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * The Android half of the tunnel: what `wandererd` is on a desktop.
 *
 * It moves through the daemon's three states, and the Network Guard is the
 * VPN interface itself:
 *
 * - **Up**: an interface that takes all traffic, this app's included, and
 *   Xray reading it. Xray's own sockets are protected, so they leave beside
 *   the interface.
 * - **Holding**: an interface that takes all traffic and nothing reading it,
 *   so nothing gets out — except this app, left out of it so it can open the
 *   next session.
 * - **Off**: no interface.
 *
 * Every change of interface establishes the new one before closing the old,
 * so there is no moment without one.
 */
class TunnelService : VpnService() {

    override fun onCreate() {
        super.onCreate()
        instance = this
        Xray.setProtector(object : Protector {
            override fun protect(fd: Long): Boolean = this@TunnelService.protect(fd.toInt())
        })
        notifications()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Android stops a foreground service that does not say so promptly.
        foreground()
        // Not sticky: restarted without the app, the service would hold a
        // tunnel no session reports on.
        return START_NOT_STICKY
    }

    override fun onRevoke() {
        // Another VPN took over, or the person turned this one off in the
        // system settings: the interface is no longer ours to hold.
        Log.w(TAG, "the VPN permission was revoked")
        lifecycle.execute {
            off()
            exit = "the system took the VPN away (another VPN app, or system settings)"
        }
    }

    override fun onDestroy() {
        lifecycle.execute { off() }
        instance = null
        super.onDestroy()
    }

    // ── States ──────────────────────────────────────────────────────────────

    private fun up(config: String, node: String, allowLan: Boolean) {
        val next = establish(allowLan, includeSelf = true)
        try {
            countRun()
            Xray.start(config, next.fd.toLong(), node)
        } catch (e: Exception) {
            // Holding, as the daemon would: the guard stays.
            interfaceOf(establish(allowLan, includeSelf = false))
            runCatching { next.close() }
            state = State.HOLDING
            throw e
        }
        interfaceOf(next)
        state = State.UP
        exit = null
        update()
    }

    private fun hold(allowLan: Boolean) {
        val next = establish(allowLan, includeSelf = false)
        countRun()
        Xray.stop()
        interfaceOf(next)
        state = State.HOLDING
        update()
    }

    private fun off() {
        countRun()
        Xray.stop()
        interfaceOf(null)
        state = State.OFF
        earlier = 0L to 0L
        runCatching { stopForeground(STOP_FOREGROUND_REMOVE) }
        stopSelf()
    }

    /** The current Xray run's bytes, added to what earlier runs carried. */
    private fun countRun() {
        if (Xray.running()) {
            earlier = (earlier.first + Xray.uplink()) to (earlier.second + Xray.downlink())
        }
    }

    private fun interfaceOf(next: ParcelFileDescriptor?) {
        val old = tun
        tun = next
        runCatching { old?.close() }
    }

    private fun establish(allowLan: Boolean, includeSelf: Boolean): ParcelFileDescriptor {
        val builder = Builder()
            .setSession(getString(R.string.app_name))
            .setMtu(MTU)
            .addAddress(ADDRESS, PREFIX)
            .addDnsServer(DNS)
            // IPv6 is not tunnelled; routed in here, it goes nowhere rather
            // than around the tunnel.
            .addAddress("fd57:616e:6465:7265::1", 128)
            .addRoute("::", 0)
        for ((address, prefix) in Routes.ipv4(allowLan)) {
            builder.addRoute(address, prefix)
        }
        if (!includeSelf) {
            builder.addDisallowedApplication(packageName)
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            builder.setMetered(false)
        }
        return builder.establish()
            ?: throw IllegalStateException("the VPN permission has not been granted")
    }

    // ── Notification ────────────────────────────────────────────────────────

    private fun notifications() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        getSystemService(NotificationManager::class.java)?.createNotificationChannel(
            NotificationChannel(
                CHANNEL,
                getString(R.string.vpn_channel),
                NotificationManager.IMPORTANCE_LOW,
            )
        )
    }

    private fun notification(): Notification {
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val text = when (state) {
            State.UP -> R.string.vpn_up
            else -> R.string.vpn_holding
        }
        return NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat_wanderer)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(getString(text))
            .setContentIntent(open)
            .setOngoing(true)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    private fun foreground() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            // No foreground type exists for a VPN; special use is the one
            // documented for it.
            startForeground(NOTIFICATION, notification(), ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(NOTIFICATION, notification())
        }
    }

    private fun update() {
        getSystemService(NotificationManager::class.java)?.notify(NOTIFICATION, notification())
    }

    enum class State(val wire: String) { OFF("off"), UP("up"), HOLDING("holding") }

    /** What the Rust core reads as the daemon's `Status`. */
    data class Status(val state: String, val up: Long, val down: Long, val exit: String?)

    companion object {
        private const val TAG = "WandererTunnel"
        private const val CHANNEL = "tunnel"
        private const val NOTIFICATION = 1

        // The addresses the desktop TUN has; Xray answers DNS on the second.
        private const val ADDRESS = "198.18.0.1"
        private const val DNS = "198.18.0.2"
        private const val PREFIX = 30
        private const val MTU = 1500

        @Volatile
        private var instance: TunnelService? = null

        @Volatile
        private var state = State.OFF

        @Volatile
        private var tun: ParcelFileDescriptor? = null

        /** Bytes of earlier Xray runs since the last start from Off. */
        @Volatile
        private var earlier = 0L to 0L

        /** Why the tunnel stopped on its own, since the last connect. */
        @Volatile
        private var exit: String? = null

        /** Every change runs here, one after another. */
        private val lifecycle: ExecutorService =
            Executors.newSingleThreadExecutor { r -> Thread(r, "wanderer-tunnel") }

        /**
         * Starts the service if it is not running and waits until it has
         * been created, so the states below have a VpnService to build
         * interfaces with.
         */
        private fun service(context: android.content.Context): TunnelService {
            instance?.let { return it }
            val intent = Intent(context, TunnelService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
            val deadline = System.currentTimeMillis() + 5_000
            while (System.currentTimeMillis() < deadline) {
                instance?.let { return it }
                Thread.sleep(20)
            }
            throw IllegalStateException("the VPN service did not start")
        }

        private fun <T> run(task: () -> T): T = lifecycle.submit(Callable(task)).get()

        fun connect(context: android.content.Context, config: String, node: String, allowLan: Boolean): Status = run {
            if (state == State.OFF) earlier = 0L to 0L
            exit = null
            service(context).up(config, node, allowLan)
            status()
        }

        fun hold(context: android.content.Context, allowLan: Boolean): Status = run {
            if (state == State.OFF) earlier = 0L to 0L
            service(context).hold(allowLan)
            status()
        }

        fun disconnect(): Status = run {
            instance?.off()
            state = State.OFF
            status()
        }

        fun status(): Status {
            if (state == State.UP && !Xray.running()) {
                exit = "Xray stopped"
                state = State.HOLDING
            }
            val running = state == State.UP
            val up = earlier.first + if (running) Xray.uplink() else 0L
            val down = earlier.second + if (running) Xray.downlink() else 0L
            return Status(state.wire, up, down, exit)
        }
    }
}
