package com.syncdrop.android.data.model

import androidx.room.Entity
import androidx.room.PrimaryKey
import java.util.UUID

@Entity(tableName = "sync_profiles")
data class SyncProfile(
    @PrimaryKey val id: String = UUID.randomUUID().toString(),
    val name: String,
    val sourceUris: String,        // JSON array of SAF URI strings
    val destPath: String,          // path on USB volume e.g. "/storage/XXXX-XXXX/SyncDrop"
    val mirrorMode: Boolean = false,
    val autoSync: Boolean = false,
    val excludePatterns: String = "[\".nomedia\",\".thumbnails\"]", // JSON array
    val createdAt: Long = System.currentTimeMillis()
)
