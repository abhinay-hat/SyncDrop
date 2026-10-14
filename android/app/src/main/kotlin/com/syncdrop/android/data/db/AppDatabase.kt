package com.syncdrop.android.data.db

import androidx.room.Database
import androidx.room.RoomDatabase
import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.model.SyncRecord

@Database(
    entities = [SyncProfile::class, SyncRecord::class],
    version = 1,
    exportSchema = false
)
abstract class AppDatabase : RoomDatabase() {
    abstract fun syncProfileDao(): SyncProfileDao
    abstract fun syncRecordDao(): SyncRecordDao

    companion object {
        const val DB_NAME = "syncdrop.db"
    }
}
