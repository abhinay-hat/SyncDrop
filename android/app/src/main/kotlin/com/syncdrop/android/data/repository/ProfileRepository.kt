package com.syncdrop.android.data.repository

import com.syncdrop.android.data.db.SyncProfileDao
import com.syncdrop.android.data.model.SyncProfile
import kotlinx.coroutines.flow.Flow
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class ProfileRepository @Inject constructor(
    private val dao: SyncProfileDao
) {
    fun observeProfiles(): Flow<List<SyncProfile>> = dao.observeAll()

    suspend fun getProfile(id: String): SyncProfile? = dao.getById(id)

    suspend fun getAutoSyncProfiles(): List<SyncProfile> = dao.getAutoSyncProfiles()

    suspend fun upsert(profile: SyncProfile) = dao.insert(profile)

    suspend fun delete(profile: SyncProfile) = dao.delete(profile)

    suspend fun deleteById(id: String) = dao.deleteById(id)
}
