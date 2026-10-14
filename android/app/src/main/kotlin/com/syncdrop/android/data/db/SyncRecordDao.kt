package com.syncdrop.android.data.db

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import com.syncdrop.android.data.model.SyncRecord
import kotlinx.coroutines.flow.Flow

@Dao
interface SyncRecordDao {

    @Query("SELECT * FROM sync_records ORDER BY startedAt DESC")
    fun observeAll(): Flow<List<SyncRecord>>

    @Query("SELECT * FROM sync_records WHERE profileId = :profileId ORDER BY startedAt DESC LIMIT 1")
    suspend fun getLatestForProfile(profileId: String): SyncRecord?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(record: SyncRecord)

    @Query("DELETE FROM sync_records")
    suspend fun clearAll()
}
