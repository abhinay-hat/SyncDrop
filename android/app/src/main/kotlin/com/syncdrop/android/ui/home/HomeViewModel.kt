package com.syncdrop.android.ui.home

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.model.SyncRecord
import com.syncdrop.android.data.repository.HistoryRepository
import com.syncdrop.android.data.repository.ProfileRepository
import com.syncdrop.android.sync.SyncScheduler
import com.syncdrop.android.usb.UsbConnectionState
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import java.util.UUID
import javax.inject.Inject

data class HomeUiState(
    val profiles: List<SyncProfile> = emptyList(),
    val latestByProfile: Map<String, SyncRecord> = emptyMap(),
    val usbConnected: Boolean = false,
    val usbMountPath: String? = null,
    val usbLabel: String? = null
)

@HiltViewModel
class HomeViewModel @Inject constructor(
    application: Application,
    private val profileRepository: ProfileRepository,
    private val historyRepository: HistoryRepository
) : AndroidViewModel(application) {

    init {
        // Recompute USB state when the screen's VM is created.
        UsbConnectionState.refresh(application)
    }

    val uiState: StateFlow<HomeUiState> =
        combine(
            profileRepository.observeProfiles(),
            historyRepository.observeRecords(),
            UsbConnectionState.state
        ) { profiles, records, usb ->
            val latest = records
                .groupBy { it.profileId }
                .mapValues { (_, recs) -> recs.maxByOrNull { it.startedAt }!! }
            HomeUiState(
                profiles = profiles,
                latestByProfile = latest,
                usbConnected = usb.connected,
                usbMountPath = usb.mountPath,
                usbLabel = usb.label
            )
        }.stateIn(
            scope = viewModelScope,
            started = SharingStarted.WhileSubscribed(5_000),
            initialValue = HomeUiState()
        )

    fun deleteProfile(profile: SyncProfile) {
        viewModelScope.launch { profileRepository.delete(profile) }
    }

    /** Enqueues a sync for [profile] and returns the WorkRequest id to navigate to. */
    fun syncNow(profile: SyncProfile): UUID {
        // If the SSD remounted at a different path than the one stored on the
        // profile, rebuild the destination from the live mount + the profile's
        // folder name. Otherwise use the stored destPath as-is.
        val mount = uiState.value.usbMountPath
        val destPath = if (profile.destPath.startsWith("content://")) {
            profile.destPath
        } else if (mount != null && !profile.destPath.startsWith(mount)) {
            "$mount/" + profile.destPath.substringAfterLast('/').ifBlank { "SyncDrop" }
        } else {
            profile.destPath
        }
        return SyncScheduler.enqueueSync(getApplication(), profile.id, destPath)
    }

    fun refreshUsb() {
        UsbConnectionState.refresh(getApplication())
    }
}
