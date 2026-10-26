package com.syncdrop.android.usb

import android.content.Context
import android.os.Build
import android.os.Environment
import android.os.storage.StorageManager
import android.os.storage.StorageVolume
import java.io.File

/**
 * Resolves the mount path of a removable (USB/SSD) storage volume.
 *
 * Strategy:
 *  1. Prefer StorageManager.storageVolumes — find a removable, mounted, non-primary volume.
 *  2. Fall back to scanning /storage/ for entries that look like a volume UUID
 *     (e.g. "XXXX-XXXX") and are readable.
 */
object UsbMountHelper {

    data class MountedVolume(val path: String, val label: String?)

    fun findMountedVolume(context: Context): MountedVolume? {
        val fromManager = findViaStorageManager(context)
        if (fromManager != null) return fromManager
        return findViaStorageScan()
    }

    fun findMountPath(context: Context): String? = findMountedVolume(context)?.path

    private fun findViaStorageManager(context: Context): MountedVolume? {
        val sm = context.getSystemService(Context.STORAGE_SERVICE) as? StorageManager
            ?: return null
        val volumes: List<StorageVolume> = sm.storageVolumes
        for (vol in volumes) {
            if (vol.isPrimary) continue
            if (!vol.isRemovable) continue
            if (vol.state != Environment.MEDIA_MOUNTED) continue
            val path = resolveVolumePath(vol) ?: continue
            val label = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                vol.mediaStoreVolumeName ?: vol.getDescription(context)
            } else {
                vol.getDescription(context)
            }
            return MountedVolume(path, label)
        }
        return null
    }

    private fun resolveVolumePath(vol: StorageVolume): String? {
        // API 30+: directory is exposed directly.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            vol.directory?.absolutePath?.let { return it }
        }
        // Older APIs / fallback: reflect getPath() (hidden but historically present).
        return try {
            val method = vol.javaClass.getMethod("getPath")
            method.invoke(vol) as? String
        } catch (e: Exception) {
            null
        }
    }

    private fun findViaStorageScan(): MountedVolume? {
        val storageDir = File("/storage")
        val children = storageDir.listFiles() ?: return null
        // Prefer UUID-shaped names (e.g. "XXXX-XXXX"). Never return an arbitrary first child.
        val uuidShape = Regex("^[A-Fa-f0-9]{4}-[A-Fa-f0-9]{4}$")
        for (child in children) {
            val name = child.name
            if (name == "self" || name == "emulated") continue
            if (!uuidShape.matches(name)) continue
            if (!child.isDirectory) continue
            if (!child.canRead()) continue
            return MountedVolume(child.absolutePath, name)
        }
        return null
    }
}
