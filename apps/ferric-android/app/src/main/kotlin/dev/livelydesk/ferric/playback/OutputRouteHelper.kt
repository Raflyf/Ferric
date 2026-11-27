package dev.livelydesk.ferric.playback

import android.content.Context
import android.media.AudioDeviceInfo
import android.media.AudioManager
import android.os.Build
import androidx.mediarouter.app.SystemOutputSwitcherDialogController
import androidx.mediarouter.media.MediaRouter

/**
 * Media output route inspection and Android System Output Switcher launcher.
 * Uses Android's media routing stack (`MediaRouter` / System Output Switcher), never VoIP
 * `AudioManager.setCommunicationDevice`.
 */
object OutputRouteHelper {
    fun activeOutputLabel(context: Context): String {
        val am = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager
            ?: return "Phone Speaker"
        val outputs = am.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
        val priorityOrder = buildList {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                add(AudioDeviceInfo.TYPE_BLE_HEADSET)
            }
            add(AudioDeviceInfo.TYPE_BLUETOOTH_A2DP)
            add(AudioDeviceInfo.TYPE_USB_HEADSET)
            add(AudioDeviceInfo.TYPE_USB_DEVICE)
            add(AudioDeviceInfo.TYPE_WIRED_HEADSET)
            add(AudioDeviceInfo.TYPE_WIRED_HEADPHONES)
            add(AudioDeviceInfo.TYPE_BUILTIN_SPEAKER)
        }
        for (targetType in priorityOrder) {
            val match = outputs.firstOrNull { it.type == targetType }
            if (match != null) {
                val product = match.productName?.toString()?.trim().orEmpty()
                return when {
                    targetType == AudioDeviceInfo.TYPE_BLUETOOTH_A2DP ||
                        (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
                            targetType == AudioDeviceInfo.TYPE_BLE_HEADSET) ->
                        product.ifEmpty { "Bluetooth Audio" }
                    targetType == AudioDeviceInfo.TYPE_USB_HEADSET ||
                        targetType == AudioDeviceInfo.TYPE_USB_DEVICE ->
                        product.ifEmpty { "USB Audio DAC" }
                    targetType == AudioDeviceInfo.TYPE_WIRED_HEADSET ||
                        targetType == AudioDeviceInfo.TYPE_WIRED_HEADPHONES ->
                        "Wired Headset"
                    else -> "Phone Speaker"
                }
            }
        }
        return "Phone Speaker"
    }

    fun openSystemOutputSwitcher(context: Context): Boolean {
        if (SystemOutputSwitcherDialogController.showDialog(context)) {
            return true
        }
        // Fallback: ensure MediaRouter instance is awake for media route discovery.
        MediaRouter.getInstance(context)
        return false
    }
}
