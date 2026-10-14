package com.syncdrop.android.data.repository

import com.syncdrop.android.data.db.SyncRecordDao
import com.syncdrop.android.data.model.SyncRecord
import kotlinx.coroutines.flow.Flow
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class HistoryRepository @Inject constructor(
    private val dao: SyncRecordDao
) {
    fun observeRecords(): Flow<List<SyncRecord>> = dao.observeAll()

    suspend fun latestForProfile(profileId: String): SyncRecord? =
        dao.getLatestForProfile(profileId)

    suspend fun add(record: SyncRecord) = dao.insert(record)

    suspend fun clear() = dao.clearAll()
}
