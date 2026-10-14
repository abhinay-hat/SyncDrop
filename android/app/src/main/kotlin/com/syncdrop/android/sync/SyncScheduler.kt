package com.syncdrop.android.sync

import android.content.Context
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.workDataOf
import java.util.UUID

/** Helper to enqueue a one-time sync and return its WorkRequest id. */
object SyncScheduler {

    fun enqueueSync(context: Context, profileId: String, destPath: String): UUID {
        val request = OneTimeWorkRequestBuilder<SyncWorker>()
            .setInputData(
                workDataOf(
                    SyncWorker.KEY_PROFILE_ID to profileId,
                    SyncWorker.KEY_USB_DEST_PATH to destPath
                )
            )
            .addTag("sync:$profileId")
            .build()
        WorkManager.getInstance(context).enqueue(request)
        return request.id
    }

    fun cancel(context: Context, workId: UUID) {
        WorkManager.getInstance(context).cancelWorkById(workId)
    }
}
