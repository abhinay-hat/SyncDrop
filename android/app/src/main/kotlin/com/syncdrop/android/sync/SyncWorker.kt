package com.syncdrop.android.sync

import android.app.Notification
import android.content.Context
import android.content.pm.ServiceInfo
import android.net.Uri
import androidx.core.app.NotificationCompat
import androidx.hilt.work.HiltWorker
import androidx.work.CoroutineWorker
import androidx.work.ForegroundInfo
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import com.syncdrop.android.R
import com.syncdrop.android.SyncDropApplication
import com.syncdrop.android.data.model.SyncRecord
import com.syncdrop.android.data.repository.HistoryRepository
import com.syncdrop.android.data.repository.JsonStringList
import com.syncdrop.android.data.repository.ProfileRepository
import dagger.assisted.Assisted
import dagger.assisted.AssistedInject
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.ensureActive
import kotlin.coroutines.coroutineContext

@HiltWorker
class SyncWorker @AssistedInject constructor(
    @Assisted appContext: Context,
    @Assisted params: WorkerParameters,
    private val profileRepository: ProfileRepository,
    private val historyRepository: HistoryRepository
) : CoroutineWorker(appContext, params) {

    override suspend fun doWork(): Result = coroutineScope {
        val profileId = inputData.getString(KEY_PROFILE_ID)
            ?: return@coroutineScope Result.failure()
        val destOverride = inputData.getString(KEY_USB_DEST_PATH)

        val profile = profileRepository.getProfile(profileId)
            ?: return@coroutineScope Result.failure()

        val destPath = destOverride ?: profile.destPath
        val startedAt = System.currentTimeMillis()

        setForeground(buildForegroundInfo(0, 0, ""))

        val excludes = JsonStringList.decode(profile.excludePatterns)
        val sourceUris = JsonStringList.decode(profile.sourceUris)

        // 1) Walk all sources into one combined map.
        val source = LinkedHashMap<String, FileEntry>()
        for (uriString in sourceUris) {
            val uri = Uri.parse(uriString)
            source.putAll(FileComparer.walkSource(applicationContext, uri, excludes))
        }

        // 2) Walk destination and diff.
        val dest = FileComparer.walkDest(destPath, applicationContext)
        val diff = FileComparer.diff(source, dest, profile.mirrorMode)

        val total = diff.toCopy.size
        var copied = 0
        var bytes = 0L
        var failure: String? = null

        try {
            // 3) Copy.
            for (entry in diff.toCopy) {
                coroutineContext.ensureActive() // honor cancellation
                setProgress(
                    workDataOf(
                        KEY_FILES_DONE to copied,
                        KEY_FILES_TOTAL to total,
                        KEY_CURRENT_FILE to entry.name
                    )
                )
                setForeground(buildForegroundInfo(copied, total, entry.name))
                val ok = FileCopier.copy(entry, destPath, applicationContext)
                if (ok) {
                    copied++
                    bytes += entry.size
                } else if (failure == null) {
                    failure = "Failed to copy ${entry.relativePath}"
                }
            }

            // 4) Mirror deletions.
            if (profile.mirrorMode) {
                for (rel in diff.toDelete) {
                    coroutineContext.ensureActive()
                    FileCopier.delete(rel, destPath, applicationContext)
                }
            }
        } catch (e: Exception) {
            failure = e.message ?: "Sync interrupted"
        }

        val succeeded = failure == null
        historyRepository.add(
            SyncRecord(
                profileId = profileId,
                startedAt = startedAt,
                finishedAt = System.currentTimeMillis(),
                filesCopied = copied,
                bytesTransferred = bytes,
                succeeded = succeeded,
                errorMessage = failure
            )
        )

        if (succeeded) {
            Result.success(
                workDataOf(KEY_FILES_DONE to copied, KEY_FILES_TOTAL to total)
            )
        } else {
            Result.failure(workDataOf(KEY_ERROR to (failure ?: "Unknown error")))
        }
    }

    private fun buildForegroundInfo(done: Int, total: Int, current: String): ForegroundInfo {
        val text = if (total > 0) "$done / $total — $current" else "Preparing…"
        val notification: Notification = NotificationCompat.Builder(
            applicationContext,
            SyncDropApplication.SYNC_CHANNEL_ID
        )
            .setContentTitle(applicationContext.getString(R.string.app_name))
            .setContentText(text)
            .setSmallIcon(android.R.drawable.stat_sys_upload)
            .setOngoing(true)
            .setProgress(total.coerceAtLeast(1), done, total == 0)
            .build()

        return if (android.os.Build.VERSION.SDK_INT >= 34) {
            ForegroundInfo(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            ForegroundInfo(NOTIFICATION_ID, notification)
        }
    }

    companion object {
        const val KEY_PROFILE_ID = "PROFILE_ID"
        const val KEY_USB_DEST_PATH = "USB_DEST_PATH"
        const val KEY_FILES_DONE = "filesDone"
        const val KEY_FILES_TOTAL = "filesTotal"
        const val KEY_CURRENT_FILE = "currentFile"
        const val KEY_ERROR = "error"
        const val NOTIFICATION_ID = 4201
    }
}
