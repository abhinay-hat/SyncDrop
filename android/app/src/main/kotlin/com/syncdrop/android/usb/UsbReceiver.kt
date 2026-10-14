package com.syncdrop.android.usb

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.util.Log

/**
 * Receives USB attach/detach broadcasts and republishes a normalized
 * local broadcast that the UI layer observes (see UsbConnectionState).
 */
class UsbReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            UsbManager.ACTION_USB_DEVICE_ATTACHED -> {
                val device: UsbDevice? = if (android.os.Build.VERSION.SDK_INT >= 33) {
                    intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)
                }
                val isStorage = device != null && UsbDeviceChecker.isMassStorage(device)
                Log.d(TAG, "USB attached: ${device?.deviceName} massStorage=$isStorage")
                if (isStorage) {
                    UsbConnectionState.notifyChanged(context, attached = true)
                }
            }

            UsbManager.ACTION_USB_DEVICE_DETACHED -> {
                Log.d(TAG, "USB detached")
                UsbConnectionState.notifyChanged(context, attached = false)
            }
        }
    }

    companion object {
        private const val TAG = "UsbReceiver"
    }
}
