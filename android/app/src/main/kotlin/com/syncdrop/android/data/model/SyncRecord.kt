package com.syncdrop.android.data.model

import androidx.room.Entity
import androidx.room.PrimaryKey
import java.util.UUID

@Entity(tableName = "sync_records")
data class SyncRecord(
    @PrimaryKey val id: String = UUID.randomUUID().toString(),
    val profileId: String,
    val startedAt: Long,
    val finishedAt: Long,
    val filesCopied: Int,
    val bytesTransferred: Long,
    val succeeded: Boolean,
    val errorMessage: String? = null
)
