package com.syncdrop.android.usb

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Process-wide observable USB connection state. The receiver pushes attach/detach
 * events here; ViewModels collect [state]. On each change we also recompute the
 * resolved mount path so the UI can auto-fill destination paths.
 */
object UsbConnectionState {

    data class State(val connected: Boolean, val mountPath: String?, val label: String?)

    private val _state = MutableStateFlow(State(connected = false, mountPath = null, label = null))
    val state: StateFlow<State> = _state.asStateFlow()

    /** Recompute from the system at startup or on demand. */
    fun refresh(context: Context) {
        val vol = UsbMountHelper.findMountedVolume(context.applicationContext)
        _state.value = State(
            connected = vol != null,
            mountPath = vol?.path,
            label = vol?.label
        )
    }

    /** Called by UsbReceiver on attach/detach. */
    fun notifyChanged(context: Context, attached: Boolean) {
        if (attached) {
            refresh(context)
        } else {
            _state.value = State(connected = false, mountPath = null, label = null)
        }
    }
}
