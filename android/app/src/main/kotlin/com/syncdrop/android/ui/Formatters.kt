package com.syncdrop.android.ui

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

object Formatters {

    private val dateFmt = SimpleDateFormat("MMM d, yyyy HH:mm", Locale.getDefault())

    fun formatDate(epochMillis: Long): String =
        if (epochMillis <= 0L) "—" else dateFmt.format(Date(epochMillis))

    fun formatBytes(bytes: Long): String {
        if (bytes < 1024) return "$bytes B"
        val units = listOf("KB", "MB", "GB", "TB")
        var value = bytes.toDouble() / 1024.0
        var idx = 0
        while (value >= 1024.0 && idx < units.size - 1) {
            value /= 1024.0
            idx++
        }
        return String.format(Locale.getDefault(), "%.1f %s", value, units[idx])
    }

    fun formatDuration(startMillis: Long, endMillis: Long): String {
        val secs = ((endMillis - startMillis) / 1000).coerceAtLeast(0)
        val m = secs / 60
        val s = secs % 60
        return if (m > 0) "${m}m ${s}s" else "${s}s"
    }
}
