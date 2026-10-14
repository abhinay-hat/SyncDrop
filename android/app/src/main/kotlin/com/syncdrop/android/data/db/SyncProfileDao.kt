package com.syncdrop.android.data.db

import androidx.room.Dao
import androidx.room.Delete
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import androidx.room.Update
import com.syncdrop.android.data.model.SyncProfile
import kotlinx.coroutines.flow.Flow

@Dao
interface SyncProfileDao {

    @Query("SELECT * FROM sync_profiles ORDER BY createdAt DESC")
    fun observeAll(): Flow<List<SyncProfile>>

    @Query("SELECT * FROM sync_profiles WHERE id = :id")
    suspend fun getById(id: String): SyncProfile?

    @Query("SELECT * FROM sync_profiles WHERE autoSync = 1")
    suspend fun getAutoSyncProfiles(): List<SyncProfile>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(profile: SyncProfile)

    @Update
    suspend fun update(profile: SyncProfile)

    @Delete
    suspend fun delete(profile: SyncProfile)

    @Query("DELETE FROM sync_profiles WHERE id = :id")
    suspend fun deleteById(id: String)
}
