package dev.livelydesk.ferric.connect

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executors
import uniffi.ferric_mobile_ffi.FfiDeviceKind

private const val FERRIC_NSD_TYPE = "_ferric-connect._udp."

/**
 * Android NSD (mDNS) advertiser & discoverer for Zero-Pairing Ferric Connect.
 * Acquires a bounded [WifiManager.MulticastLock] only during the initial 10-second discovery burst
 * or while the user has the Connect sheet open so standby battery drain remains near zero.
 */
class ConnectNsdHelper(
    private val context: Context,
    private val onPeerResolved: (
        deviceId: String,
        deviceName: String,
        deviceKind: FfiDeviceKind,
        ip: String,
        tcpPort: Int,
        roomTagsHex: String,
    ) -> Unit,
) {
    private val nsdManager: NsdManager? =
        context.getSystemService(Context.NSD_SERVICE) as? NsdManager
    private val wifiManager: WifiManager? =
        context.applicationContext.getSystemService(Context.WIFI_SERVICE) as? WifiManager
    private val mainHandler = Handler(Looper.getMainLooper())
    private val callbackExecutor = Executors.newSingleThreadExecutor()

    private var multicastLock: WifiManager.MulticastLock? = null
    private var registrationListener: NsdManager.RegistrationListener? = null
    private var discoveryListener: NsdManager.DiscoveryListener? = null
    private var lastRegisteredKey: String? = null
    private var localDeviceId: String = ""
    private var isDiscoveryRunning = false
    private var boostPinned = false

    private val releaseBurstRunnable = Runnable {
        if (!boostPinned) {
            releaseMulticastLock()
        }
    }

    @Synchronized
    fun syncRegistration(
        deviceId: String,
        deviceName: String,
        tcpPort: Int,
        roomTagsHex: String,
    ) {
        val mgr = nsdManager ?: return
        if (tcpPort < 1024 || deviceId.isBlank()) return
        localDeviceId = deviceId
        val key = "$deviceId|$deviceName|$tcpPort|$roomTagsHex"
        if (key == lastRegisteredKey) {
            ensureDiscoveryStarted()
            return
        }
        lastRegisteredKey = key
        unregisterServiceLocked(mgr)

        val info = NsdServiceInfo().apply {
            serviceName = "Ferric-${deviceId.takeLast(8)}"
            serviceType = FERRIC_NSD_TYPE
            port = tcpPort
            setAttribute("id", deviceId.take(64))
            setAttribute("name", deviceName.take(48))
            setAttribute("kind", "phone")
            setAttribute("tags", roomTagsHex.take(72))
        }
        val listener = object : NsdManager.RegistrationListener {
            override fun onServiceRegistered(serviceInfo: NsdServiceInfo) {}
            override fun onRegistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {}
            override fun onServiceUnregistered(serviceInfo: NsdServiceInfo) {}
            override fun onUnregistrationFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {}
        }
        registrationListener = listener
        runCatching {
            mgr.registerService(info, NsdManager.PROTOCOL_DNS_SD, listener)
        }
        ensureDiscoveryStarted()
        triggerInitialBurstLock()
    }

    @Synchronized
    fun setSheetDiscoveryBoost(open: Boolean) {
        boostPinned = open
        if (open) {
            acquireMulticastLock()
            ensureDiscoveryStarted()
        } else {
            mainHandler.removeCallbacks(releaseBurstRunnable)
            releaseMulticastLock()
        }
    }

    @Synchronized
    fun shutdown() {
        mainHandler.removeCallbacks(releaseBurstRunnable)
        boostPinned = false
        val mgr = nsdManager
        if (mgr != null) {
            unregisterServiceLocked(mgr)
            discoveryListener?.let { listener ->
                runCatching { mgr.stopServiceDiscovery(listener) }
            }
        }
        discoveryListener = null
        isDiscoveryRunning = false
        lastRegisteredKey = null
        releaseMulticastLock()
        callbackExecutor.shutdownNow()
    }

    private fun ensureDiscoveryStarted() {
        val mgr = nsdManager ?: return
        if (isDiscoveryRunning) return
        val listener = object : NsdManager.DiscoveryListener {
            override fun onDiscoveryStarted(regType: String) {
                isDiscoveryRunning = true
            }

            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                if (!serviceInfo.serviceType.contains("_ferric-connect")) return
                resolvePeerService(mgr, serviceInfo)
            }

            override fun onServiceLost(serviceInfo: NsdServiceInfo) {}

            override fun onDiscoveryStopped(serviceType: String) {
                isDiscoveryRunning = false
            }

            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                isDiscoveryRunning = false
                runCatching { mgr.stopServiceDiscovery(this) }
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                isDiscoveryRunning = false
            }
        }
        discoveryListener = listener
        runCatching {
            mgr.discoverServices(FERRIC_NSD_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
        }
    }

    private fun resolvePeerService(mgr: NsdManager, serviceInfo: NsdServiceInfo) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            val cb = object : NsdManager.ServiceInfoCallback {
                override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {}
                override fun onServiceUpdated(resolved: NsdServiceInfo) {
                    emitResolved(resolved)
                    runCatching { mgr.unregisterServiceInfoCallback(this) }
                }

                override fun onServiceLost() {}
                override fun onServiceInfoCallbackUnregistered() {}
            }
            runCatching {
                mgr.registerServiceInfoCallback(serviceInfo, callbackExecutor, cb)
            }
        } else {
            @Suppress("DEPRECATION")
            runCatching {
                mgr.resolveService(
                    serviceInfo,
                    object : NsdManager.ResolveListener {
                        override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) {}
                        override fun onServiceResolved(resolved: NsdServiceInfo) {
                            emitResolved(resolved)
                        }
                    },
                )
            }
        }
    }

    private fun emitResolved(resolved: NsdServiceInfo) {
        val attrs = resolved.attributes
        val id = attrs["id"]?.decodeToString()?.trim().orEmpty()
        if (id.isBlank() || id == localDeviceId) return
        val name = attrs["name"]?.decodeToString()?.trim().orEmpty()
            .ifBlank { resolved.serviceName }
        val kindStr = attrs["kind"]?.decodeToString()?.trim().orEmpty()
        val kind = if (kindStr.equals("phone", ignoreCase = true)) {
            FfiDeviceKind.PHONE
        } else {
            FfiDeviceKind.DESKTOP
        }
        val tagsHex = attrs["tags"]?.decodeToString()?.trim().orEmpty()
        val port = resolved.port
        val hostAddr = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            resolved.hostAddresses.firstOrNull()?.hostAddress
        } else {
            @Suppress("DEPRECATION")
            resolved.host?.hostAddress
        } ?: return
        if (port >= 1024) {
            onPeerResolved(id, name, kind, hostAddr, port, tagsHex)
        }
    }

    private fun unregisterServiceLocked(mgr: NsdManager) {
        registrationListener?.let { listener ->
            runCatching { mgr.unregisterService(listener) }
        }
        registrationListener = null
    }

    private fun triggerInitialBurstLock() {
        acquireMulticastLock()
        mainHandler.removeCallbacks(releaseBurstRunnable)
        mainHandler.postDelayed(releaseBurstRunnable, 10_000L)
    }

    private fun acquireMulticastLock() {
        val wm = wifiManager ?: return
        runCatching {
            val lock = multicastLock ?: wm.createMulticastLock("ferric-connect-nsd").apply {
                setReferenceCounted(false)
                multicastLock = this
            }
            if (!lock.isHeld) {
                lock.acquire()
            }
        }
    }

    private fun releaseMulticastLock() {
        runCatching {
            multicastLock?.let { lock ->
                if (lock.isHeld) lock.release()
            }
        }
    }
}
