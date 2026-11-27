package dev.livelydesk.ferric.connect

import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import java.net.Inet4Address
import java.util.ArrayDeque
import java.util.Collections
import java.util.concurrent.Executors
import uniffi.ferric_mobile_ffi.FfiDeviceKind

private const val FERRIC_NSD_TYPE = "_ferric-connect._udp."

/**
 * Android NSD (mDNS) advertiser & discoverer for Zero-Pairing Ferric Connect.
 * Acquires a bounded [WifiManager.MulticastLock] only during the initial 10-second discovery burst
 * or while the user has the Connect sheet open, tracks API 34+ callbacks cleanly, serializes
 * API 28-33 resolves, prefers IPv4 addresses, and pauses discovery when backgrounded.
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

    private val activeInfoCallbacks = Collections.synchronizedSet(mutableSetOf<Any>())
    private val legacyResolveQueue = ArrayDeque<NsdServiceInfo>()
    private var legacyResolving = false

    private var multicastLock: WifiManager.MulticastLock? = null
    private var registrationListener: NsdManager.RegistrationListener? = null
    private var discoveryListener: NsdManager.DiscoveryListener? = null
    private var lastRegisteredKey: String? = null
    private var localDeviceId: String = ""
    private var isDiscoveryRunning = false
    private var boostPinned = false
    private var isForeground = true

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
            if (isForeground) ensureDiscoveryStarted()
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
        if (isForeground) {
            ensureDiscoveryStarted()
            triggerInitialBurstLock()
        }
    }

    @Synchronized
    fun setAppForeground(foreground: Boolean) {
        isForeground = foreground
        val mgr = nsdManager ?: return
        if (foreground) {
            if (lastRegisteredKey != null) {
                ensureDiscoveryStarted()
            }
        } else {
            stopDiscoveryLocked(mgr)
            if (!boostPinned) {
                mainHandler.removeCallbacks(releaseBurstRunnable)
                releaseMulticastLock()
            }
        }
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
            stopDiscoveryLocked(mgr)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                val snapshot = synchronized(activeInfoCallbacks) {
                    val copy = activeInfoCallbacks.toList()
                    activeInfoCallbacks.clear()
                    copy
                }
                snapshot.forEach { cb ->
                    (cb as? NsdManager.ServiceInfoCallback)?.let {
                        runCatching { mgr.unregisterServiceInfoCallback(it) }
                    }
                }
            }
        }
        legacyResolveQueue.clear()
        legacyResolving = false
        lastRegisteredKey = null
        releaseMulticastLock()
        callbackExecutor.shutdownNow()
    }

    private fun stopDiscoveryLocked(mgr: NsdManager) {
        discoveryListener?.let { listener ->
            runCatching { mgr.stopServiceDiscovery(listener) }
        }
        discoveryListener = null
        isDiscoveryRunning = false
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
                // Never call stopServiceDiscovery when start failed; Android throws IllegalArgumentException.
                isDiscoveryRunning = false
                discoveryListener = null
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) {
                isDiscoveryRunning = false
                discoveryListener = null
            }
        }
        discoveryListener = listener
        runCatching {
            mgr.discoverServices(FERRIC_NSD_TYPE, NsdManager.PROTOCOL_DNS_SD, listener)
        }.onFailure {
            isDiscoveryRunning = false
            discoveryListener = null
        }
    }

    private fun resolvePeerService(mgr: NsdManager, serviceInfo: NsdServiceInfo) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            val cb = object : NsdManager.ServiceInfoCallback {
                override fun onServiceInfoCallbackRegistrationFailed(errorCode: Int) {
                    activeInfoCallbacks.remove(this)
                }

                override fun onServiceUpdated(resolved: NsdServiceInfo) {
                    emitResolved(resolved)
                    activeInfoCallbacks.remove(this)
                    runCatching { mgr.unregisterServiceInfoCallback(this) }
                }

                override fun onServiceLost() {
                    activeInfoCallbacks.remove(this)
                    runCatching { mgr.unregisterServiceInfoCallback(this) }
                }

                override fun onServiceInfoCallbackUnregistered() {
                    activeInfoCallbacks.remove(this)
                }
            }
            activeInfoCallbacks.add(cb)
            runCatching {
                mgr.registerServiceInfoCallback(serviceInfo, callbackExecutor, cb)
            }.onFailure {
                activeInfoCallbacks.remove(cb)
            }
        } else {
            synchronized(this) {
                if (legacyResolveQueue.size < 16) {
                    legacyResolveQueue.addLast(serviceInfo)
                }
                if (!legacyResolving) {
                    drainNextLegacyResolveLocked(mgr)
                }
            }
        }
    }

    private fun drainNextLegacyResolveLocked(mgr: NsdManager) {
        val next = legacyResolveQueue.pollFirst()
        if (next == null) {
            legacyResolving = false
            return
        }
        legacyResolving = true
        @Suppress("DEPRECATION")
        runCatching {
            mgr.resolveService(
                next,
                object : NsdManager.ResolveListener {
                    override fun onResolveFailed(info: NsdServiceInfo, errorCode: Int) {
                        synchronized(this@ConnectNsdHelper) {
                            drainNextLegacyResolveLocked(mgr)
                        }
                    }

                    override fun onServiceResolved(resolved: NsdServiceInfo) {
                        emitResolved(resolved)
                        synchronized(this@ConnectNsdHelper) {
                            drainNextLegacyResolveLocked(mgr)
                        }
                    }
                },
            )
        }.onFailure {
            drainNextLegacyResolveLocked(mgr)
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
            val addrs = resolved.hostAddresses
            (addrs.firstOrNull { it is Inet4Address } ?: addrs.firstOrNull { !it.isLinkLocalAddress } ?: addrs.firstOrNull())
                ?.hostAddress
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
