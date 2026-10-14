package com.syncdrop.android.usb

import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbInterface
import com.google.common.truth.Truth.assertThat
import io.mockk.every
import io.mockk.mockk
import org.junit.Test

class UsbDeviceCheckerTest {

    @Test
    fun `device-level mass storage class is detected`() {
        val device = mockk<UsbDevice> {
            every { deviceClass } returns UsbConstants.USB_CLASS_MASS_STORAGE
            every { interfaceCount } returns 0
        }
        assertThat(UsbDeviceChecker.isMassStorage(device)).isTrue()
    }

    @Test
    fun `interface-level mass storage class is detected`() {
        val iface = mockk<UsbInterface> {
            every { interfaceClass } returns UsbConstants.USB_CLASS_MASS_STORAGE
        }
        val device = mockk<UsbDevice> {
            every { deviceClass } returns UsbConstants.USB_CLASS_PER_INTERFACE
            every { interfaceCount } returns 1
            every { getInterface(0) } returns iface
        }
        assertThat(UsbDeviceChecker.isMassStorage(device)).isTrue()
    }

    @Test
    fun `non storage device is not detected`() {
        val iface = mockk<UsbInterface> {
            every { interfaceClass } returns UsbConstants.USB_CLASS_HID
        }
        val device = mockk<UsbDevice> {
            every { deviceClass } returns UsbConstants.USB_CLASS_HID
            every { interfaceCount } returns 1
            every { getInterface(0) } returns iface
        }
        assertThat(UsbDeviceChecker.isMassStorage(device)).isFalse()
    }
}
