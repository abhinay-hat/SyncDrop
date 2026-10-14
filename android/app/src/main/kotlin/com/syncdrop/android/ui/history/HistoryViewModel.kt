package com.syncdrop.android.ui.history

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.model.SyncRecord
import com.syncdrop.android.data.repository.HistoryRepository
import com.syncdrop.android.data.repository.ProfileRepository
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import javax.inject.Inject

data class HistoryRow(
    val record: SyncRecord,
    val profileName: String
)

data class HistoryUiState(
    val rows: List<HistoryRow> = emptyList()
)

@HiltViewModel
class HistoryViewModel @Inject constructor(
    private val historyRepository: HistoryRepository,
    private val profileRepository: ProfileRepository
) : ViewModel() {

    val uiState: StateFlow<HistoryUiState> =
        combine(
            historyRepository.observeRecords(),
            profileRepository.observeProfiles()
        ) { records, profiles ->
            val nameById: Map<String, String> =
                profiles.associate { p: SyncProfile -> p.id to p.name }
            HistoryUiState(
                rows = records.map { rec ->
                    HistoryRow(record = rec, profileName = nameById[rec.profileId] ?: "(deleted)")
                }
            )
        }.stateIn(
            scope = viewModelScope,
            started = SharingStarted.WhileSubscribed(5_000),
            initialValue = HistoryUiState()
        )

    fun clearHistory() {
        viewModelScope.launch { historyRepository.clear() }
    }
}
