package com.syncdrop.android.usb

import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice

/**
 * Determines whether a connected USB device is a mass-storage device
 * (class 0x08), either at the device level or on any of its interfaces.
 */
object UsbDeviceChecker {

    private const val USB_CLASS_MASS_STORAGE = UsbConstants.USB_CLASS_MASS_STORAGE // 0x08

    fun isMassStorage(device: UsbDevice): Boolean {
        if (device.deviceClass == USB_CLASS_MASS_STORAGE) return true
        for (i in 0 until device.interfaceCount) {
            if (device.getInterface(i).interfaceClass == USB_CLASS_MASS_STORAGE) {
                return true
            }
        }
        return false
    }
}
