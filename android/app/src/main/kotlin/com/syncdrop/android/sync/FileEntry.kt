package com.syncdrop.android.sync

import android.net.Uri

data class FileEntry(
    val uri: Uri,
    val name: String,
    val size: Long,
    val lastModified: Long,
    val relativePath: String
)

data class SyncDiff(
    val toCopy: List<FileEntry>,
    val toDelete: List<String>
)

/** Lightweight, Android-free descriptor of a destination file for diffing. */
data class DestEntry(
    val relativePath: String,
    val size: Long,
    val lastModified: Long
)
