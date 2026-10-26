package com.syncdrop.android.sync

import android.content.Context
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import java.io.File

/**
 * Compares a source SAF tree against a destination File tree and produces a SyncDiff.
 *
 * The pure diff algorithm ([diff]) is Android-free and unit-tested. The walking
 * methods ([walkSource], [walkDest]) adapt the platform APIs into plain maps.
 */
object FileComparer {

    /**
     * Pure diff. A source file is copied when:
     *  - the destination has no file at that relativePath, OR
     *  - dest.lastModified < source.lastModified, OR
     *  - dest.size != source.size.
     * When [mirrorMode] is true, destination files absent from the source are deleted.
     */
    fun diff(
        source: Map<String, FileEntry>,
        dest: Map<String, DestEntry>,
        mirrorMode: Boolean
    ): SyncDiff {
        val toCopy = source.values.filter { src ->
            val d = dest[src.relativePath]
            d == null || d.lastModified < src.lastModified || d.size != src.size
        }
        val toDelete = if (mirrorMode) {
            dest.keys.filter { it !in source.keys }
        } else {
            emptyList()
        }
        return SyncDiff(toCopy = toCopy, toDelete = toDelete)
    }

    /** True if [name] matches any exclusion pattern (exact name or "*.ext" glob). */
    fun isExcluded(name: String, excludePatterns: List<String>): Boolean {
        for (pattern in excludePatterns) {
            if (pattern.isEmpty()) continue
            if (pattern.startsWith("*.")) {
                val ext = pattern.substring(1) // ".ext"
                if (name.endsWith(ext, ignoreCase = true)) return true
            } else if (name.equals(pattern, ignoreCase = false)) {
                return true
            }
        }
        return false
    }

    /**
     * Walks a SAF source tree rooted at [rootUri], returning relativePath -> FileEntry.
     * [rootPrefix] is prepended to relative paths so multiple sources can share a dest
     * without collisions (uses the root document's display name).
     */
    fun walkSource(
        context: Context,
        rootUri: Uri,
        excludePatterns: List<String>
    ): Map<String, FileEntry> {
        val result = LinkedHashMap<String, FileEntry>()
        val root = DocumentFile.fromTreeUri(context, rootUri) ?: return result
        val rootName = root.name ?: "source"
        walkSourceRecursive(root, rootName, excludePatterns, result)
        return result
    }

    private fun walkSourceRecursive(
        dir: DocumentFile,
        relativeDir: String,
        excludePatterns: List<String>,
        out: MutableMap<String, FileEntry>
    ) {
        for (child in dir.listFiles()) {
            val name = child.name ?: continue
            if (name == "." || name == ".." || name.contains('/') || name.contains('\\')) continue
            if (isExcluded(name, excludePatterns)) continue
            val childRelative = "$relativeDir/$name"
            if (child.isDirectory) {
                walkSourceRecursive(child, childRelative, excludePatterns, out)
            } else {
                out[childRelative] = FileEntry(
                    uri = child.uri,
                    name = name,
                    size = child.length(),
                    lastModified = child.lastModified(),
                    relativePath = childRelative
                )
            }
        }
    }

    /**
     * Walks the destination tree under [destRoot] (either a SAF content:// URI or a local File path),
     * returning relativePath -> DestEntry.
     */
    fun walkDest(destRoot: String, context: Context? = null): Map<String, DestEntry> {
        if (destRoot.startsWith("content://") && context != null) {
            val uri = Uri.parse(destRoot)
            return walkDestTree(context, uri)
        }
        val result = LinkedHashMap<String, DestEntry>()
        val root = File(destRoot)
        if (!root.exists() || !root.isDirectory) return result
        walkDestRecursive(root, root, result)
        return result
    }

    /** Walks a SAF destination tree rooted at [rootUri], returning relativePath -> DestEntry. */
    fun walkDestTree(context: Context, rootUri: Uri): Map<String, DestEntry> {
        val result = LinkedHashMap<String, DestEntry>()
        val root = DocumentFile.fromTreeUri(context, rootUri) ?: return result
        walkDestTreeRecursive(root, "", result)
        return result
    }

    private fun walkDestTreeRecursive(
        dir: DocumentFile,
        relativeDir: String,
        out: MutableMap<String, DestEntry>
    ) {
        for (child in dir.listFiles()) {
            val name = child.name ?: continue
            if (name == "." || name == ".." || name.contains('/') || name.contains('\\')) continue
            val childRelative = if (relativeDir.isEmpty()) name else "$relativeDir/$name"
            if (child.isDirectory) {
                walkDestTreeRecursive(child, childRelative, out)
            } else {
                out[childRelative] = DestEntry(
                    relativePath = childRelative,
                    size = child.length(),
                    lastModified = child.lastModified()
                )
            }
        }
    }

    private fun walkDestRecursive(root: File, dir: File, out: MutableMap<String, DestEntry>) {
        val children = dir.listFiles() ?: return
        for (child in children) {
            if (child.isDirectory) {
                walkDestRecursive(root, child, out)
            } else {
                val relative = root.toURI().relativize(child.toURI()).path
                out[relative] = DestEntry(
                    relativePath = relative,
                    size = child.length(),
                    lastModified = child.lastModified()
                )
            }
        }
    }
}
