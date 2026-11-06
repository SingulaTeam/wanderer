package network.wanderer.app

import android.app.Activity
import android.net.VpnService
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import network.wanderer.app.vpn.TunnelService
import java.util.concurrent.Executors

@InvokeArg
class ConnectArgs {
    lateinit var config: String
    /** The node's address, "ip:port": the tunnel's sockets to it bypass the VPN. */
    lateinit var node: String
    var allowLan: Boolean = false
}

@InvokeArg
class RelayArgs {
    lateinit var config: String
}

@InvokeArg
class HoldArgs {
    var allowLan: Boolean = false
}

/**
 * What the Rust core calls in place of the daemon's socket (see
 * `src/android.rs`): the same requests, answered with the same status.
 */
@TauriPlugin
class TunnelPlugin(private val activity: Activity) : Plugin(activity) {

    /** Calls wait on the service's own queue; never on the main thread. */
    private val calls = Executors.newCachedThreadPool()

    /** Asks for the VPN permission, if the app does not hold it. */
    @Command
    fun prepare(invoke: Invoke) {
        val intent = VpnService.prepare(activity)
        if (intent == null) {
            invoke.resolve(JSObject().put("granted", true))
        } else {
            startActivityForResult(invoke, intent, "prepared")
        }
    }

    @ActivityCallback
    fun prepared(invoke: Invoke, result: ActivityResult) {
        invoke.resolve(JSObject().put("granted", result.resultCode == Activity.RESULT_OK))
    }

    @Command
    fun connect(invoke: Invoke) {
        val args = invoke.parseArgs(ConnectArgs::class.java)
        answer(invoke) { TunnelService.connect(activity, args.config, args.node, args.allowLan) }
    }

    /**
     * Runs the relay to Control Planes' fronts, in place of any running.
     * It needs no VPN, and its sockets are not protected: while the tunnel
     * is up they go through it like the app's own.
     */
    @Command
    fun startRelay(invoke: Invoke) {
        val args = invoke.parseArgs(RelayArgs::class.java)
        calls.execute {
            try {
                network.wanderer.app.xray.Xray.startRelay(args.config)
                invoke.resolve(JSObject())
            } catch (e: Exception) {
                invoke.reject(e.message ?: e.toString())
            }
        }
    }

    @Command
    fun hold(invoke: Invoke) {
        val args = invoke.parseArgs(HoldArgs::class.java)
        answer(invoke) { TunnelService.hold(activity, args.allowLan) }
    }

    @Command
    fun disconnect(invoke: Invoke) {
        answer(invoke) { TunnelService.disconnect() }
    }

    @Command
    fun status(invoke: Invoke) {
        answer(invoke) { TunnelService.status() }
    }

    /**
     * Runs `call` off the main thread and answers with the status, or with
     * the failure and the status after it, as the daemon does.
     */
    private fun answer(invoke: Invoke, call: () -> TunnelService.Status) {
        calls.execute {
            val reply = try {
                JSObject().put("status", wire(call()))
            } catch (e: Exception) {
                val cause = (e as? java.util.concurrent.ExecutionException)?.cause ?: e
                JSObject()
                    .put("error", cause.message ?: cause.toString())
                    .put("status", wire(TunnelService.status()))
            }
            invoke.resolve(reply)
        }
    }

    private fun wire(s: TunnelService.Status): JSObject =
        JSObject()
            .put("state", s.state)
            .put("up", s.up)
            .put("down", s.down)
            .put("exit", s.exit)
}
