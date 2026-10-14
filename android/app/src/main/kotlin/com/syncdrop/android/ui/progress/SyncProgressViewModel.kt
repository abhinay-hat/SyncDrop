package com.syncdrop.android.ui.progress

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.work.WorkInfo
import androidx.work.WorkManager
import com.syncdrop.android.sync.SyncWorker
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.util.UUID
import javax.inject.Inject

data class SyncProgressUiState(
    val filesDone: Int = 0,
    val filesTotal: Int = 0,
    val currentFile: String = "",
    val finished: Boolean = false,
    val succeeded: Boolean = false,
    val error: String? = null
)

@HiltViewModel
class SyncProgressViewModel @Inject constructor(
    application: Application
) : AndroidViewModel(application) {

    private val _state = MutableStateFlow(SyncProgressUiState())
    val state: StateFlow<SyncProgressUiState> = _state.asStateFlow()

    private var workId: UUID? = null

    fun observe(id: UUID) {
        workId = id
        viewModelScope.launch {
            WorkManager.getInstance(getApplication())
                .getWorkInfoByIdFlow(id)
                .collect { info -> handle(info) }
        }
    }

    private fun handle(info: WorkInfo?) {
        if (info == null) return
        when (info.state) {
            WorkInfo.State.RUNNING -> {
                val p = info.progress
                _state.value = _state.value.copy(
                    filesDone = p.getInt(SyncWorker.KEY_FILES_DONE, _state.value.filesDone),
                    filesTotal = p.getInt(SyncWorker.KEY_FILES_TOTAL, _state.value.filesTotal),
                    currentFile = p.getString(SyncWorker.KEY_CURRENT_FILE) ?: _state.value.currentFile
                )
            }
            WorkInfo.State.SUCCEEDED -> {
                _state.value = _state.value.copy(
                    filesDone = info.outputData.getInt(SyncWorker.KEY_FILES_DONE, _state.value.filesDone),
                    filesTotal = info.outputData.getInt(SyncWorker.KEY_FILES_TOTAL, _state.value.filesTotal),
                    finished = true,
                    succeeded = true
                )
            }
            WorkInfo.State.FAILED -> {
                _state.value = _state.value.copy(
                    finished = true,
                    succeeded = false,
                    error = info.outputData.getString(SyncWorker.KEY_ERROR)
                )
            }
            WorkInfo.State.CANCELLED -> {
                _state.value = _state.value.copy(
                    finished = true,
                    succeeded = false,
                    error = "Cancelled"
                )
            }
            else -> { /* ENQUEUED, BLOCKED — keep current state */ }
        }
    }

    fun cancel() {
        workId?.let { WorkManager.getInstance(getApplication()).cancelWorkById(it) }
    }
}
