package com.syncdrop.android.sync

import android.content.Context
import android.util.Log
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import java.io.File
import java.io.FileOutputStream

/**
 * Copies a source SAF file to a destination tree (either a local File tree or a SAF DocumentFile tree).
 */
object FileCopier {

    private const val BUFFER_SIZE = 64 * 1024

    fun copy(entry: FileEntry, destRoot: String, context: Context): Boolean {
        if (destRoot.startsWith("content://")) {
            return copyToSafTree(entry, Uri.parse(destRoot), context)
        }
        val destFile = resolveUnderRoot(destRoot, entry.relativePath) ?: return false
        return try {
            destFile.parentFile?.let { parent ->
                if (!parent.exists() && !parent.mkdirs()) {
                    Log.e(TAG, "Failed to create dirs: ${parent.absolutePath}")
                    return false
                }
            }
            context.contentResolver.openInputStream(entry.uri)?.use { input ->
                FileOutputStream(destFile).use { output ->
                    val buffer = ByteArray(BUFFER_SIZE)
                    var read: Int
                    while (input.read(buffer).also { read = it } >= 0) {
                        output.write(buffer, 0, read)
                    }
                    output.flush()
                }
            } ?: run {
                Log.e(TAG, "Could not open input stream for ${entry.uri}")
                return false
            }
            // Preserve source modification time so future diffs are stable.
            if (entry.lastModified > 0) {
                destFile.setLastModified(entry.lastModified)
            }
            true
        } catch (e: Exception) {
            Log.e(TAG, "Copy failed for ${entry.relativePath}: ${e.message}", e)
            false
        }
    }

    private fun copyToSafTree(entry: FileEntry, rootUri: Uri, context: Context): Boolean {
        return try {
            val rootDoc = DocumentFile.fromTreeUri(context, rootUri) ?: return false
            val segments = safeSegments(entry.relativePath) ?: return false
            if (segments.isEmpty()) return false
            val dirSegments = segments.dropLast(1)
            val fileName = segments.last()

            var currentDir = rootDoc
            for (dir in dirSegments) {
                currentDir = currentDir.findFile(dir)
                    ?: currentDir.createDirectory(dir)
                    ?: return false
            }

            val targetDoc = currentDir.findFile(fileName)
                ?: currentDir.createFile("application/octet-stream", fileName)
                ?: return false

            context.contentResolver.openInputStream(entry.uri)?.use { input ->
                context.contentResolver.openOutputStream(targetDoc.uri, "wt")?.use { output ->
                    val buffer = ByteArray(BUFFER_SIZE)
                    var read: Int
                    while (input.read(buffer).also { read = it } >= 0) {
                        output.write(buffer, 0, read)
                    }
                    output.flush()
                }
            } ?: run {
                Log.e(TAG, "Could not open input stream for ${entry.uri}")
                return false
            }
            true
        } catch (e: Exception) {
            Log.e(TAG, "SAF copy failed for ${entry.relativePath}: ${e.message}", e)
            false
        }
    }

    /** Deletes a destination file by relative path (used in mirror mode). */
    fun delete(relativePath: String, destRoot: String, context: Context? = null): Boolean {
        if (destRoot.startsWith("content://") && context != null) {
            return deleteFromSafTree(relativePath, Uri.parse(destRoot), context)
        }
        return try {
            val f = resolveUnderRoot(destRoot, relativePath) ?: return false
            if (f.exists()) f.delete() else true
        } catch (e: Exception) {
            Log.e(TAG, "Delete failed for $relativePath: ${e.message}", e)
            false
        }
    }

    private fun deleteFromSafTree(relativePath: String, rootUri: Uri, context: Context): Boolean {
        return try {
            val rootDoc = DocumentFile.fromTreeUri(context, rootUri) ?: return true
            val segments = safeSegments(relativePath) ?: return true
            var current: DocumentFile? = rootDoc
            for (seg in segments) {
                current = current?.findFile(seg) ?: return true
            }
            current?.delete() ?: true
        } catch (e: Exception) {
            Log.e(TAG, "SAF delete failed for $relativePath: ${e.message}", e)
            false
        }
    }

    /** Reject `..` / empty segments and keep the resolved file under [destRoot]. */
    internal fun resolveUnderRoot(destRoot: String, relativePath: String): File? {
        val segments = safeSegments(relativePath) ?: return null
        val root = File(destRoot).canonicalFile
        var current = root
        for (seg in segments) {
            current = File(current, seg)
        }
        val resolved = current.canonicalFile
        val rootPath = root.path
        if (resolved == root) return null
        if (!resolved.path.startsWith(rootPath + File.separator)) return null
        return resolved
    }

    private fun safeSegments(relativePath: String): List<String>? {
        val parts = relativePath.split('/', '\\')
        if (parts.isEmpty()) return null
        val out = ArrayList<String>(parts.size)
        for (part in parts) {
            if (part.isEmpty() || part == ".") continue
            if (part == "..") return null
            out.add(part)
        }
        return out.takeIf { it.isNotEmpty() }
    }

    private const val TAG = "FileCopier"
}
