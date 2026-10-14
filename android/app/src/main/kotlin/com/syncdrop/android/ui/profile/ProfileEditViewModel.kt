package com.syncdrop.android.ui.profile

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.repository.JsonStringList
import com.syncdrop.android.data.repository.ProfileRepository
import com.syncdrop.android.usb.UsbConnectionState
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class ProfileEditUiState(
    val id: String? = null,
    val name: String = "",
    val sourceUris: List<String> = emptyList(),
    val destPath: String = "",
    val mirrorMode: Boolean = false,
    val autoSync: Boolean = false,
    val excludePatterns: List<String> = listOf(".nomedia", ".thumbnails"),
    val saved: Boolean = false
)

@HiltViewModel
class ProfileEditViewModel @Inject constructor(
    application: Application,
    private val repository: ProfileRepository
) : AndroidViewModel(application) {

    private val _state = MutableStateFlow(ProfileEditUiState())
    val state: StateFlow<ProfileEditUiState> = _state.asStateFlow()

    fun load(profileId: String?) {
        if (profileId == null) {
            // New profile: prefill dest from the connected SSD if available.
            val mount = UsbConnectionState.state.value.mountPath
            _state.update {
                it.copy(destPath = mount?.let { m -> "$m/SyncDrop" } ?: "")
            }
            return
        }
        viewModelScope.launch {
            val p = repository.getProfile(profileId) ?: return@launch
            _state.value = ProfileEditUiState(
                id = p.id,
                name = p.name,
                sourceUris = JsonStringList.decode(p.sourceUris),
                destPath = p.destPath,
                mirrorMode = p.mirrorMode,
                autoSync = p.autoSync,
                excludePatterns = JsonStringList.decode(p.excludePatterns)
            )
        }
    }

    fun onNameChange(value: String) = _state.update { it.copy(name = value) }

    fun onDestPathChange(value: String) = _state.update { it.copy(destPath = value) }

    fun onMirrorToggle(value: Boolean) = _state.update { it.copy(mirrorMode = value) }

    fun onAutoSyncToggle(value: Boolean) = _state.update { it.copy(autoSync = value) }

    fun addSource(uriString: String) = _state.update {
        if (it.sourceUris.contains(uriString)) it
        else it.copy(sourceUris = it.sourceUris + uriString)
    }

    fun removeSource(uriString: String) = _state.update {
        it.copy(sourceUris = it.sourceUris - uriString)
    }

    fun addExcludePattern(pattern: String) = _state.update {
        val trimmed = pattern.trim()
        if (trimmed.isEmpty() || it.excludePatterns.contains(trimmed)) it
        else it.copy(excludePatterns = it.excludePatterns + trimmed)
    }

    fun removeExcludePattern(pattern: String) = _state.update {
        it.copy(excludePatterns = it.excludePatterns - pattern)
    }

    fun save() {
        val s = _state.value
        if (s.name.isBlank()) return
        viewModelScope.launch {
            val profile = if (s.id != null) {
                SyncProfile(
                    id = s.id,
                    name = s.name,
                    sourceUris = JsonStringList.encode(s.sourceUris),
                    destPath = s.destPath,
                    mirrorMode = s.mirrorMode,
                    autoSync = s.autoSync,
                    excludePatterns = JsonStringList.encode(s.excludePatterns)
                )
            } else {
                SyncProfile(
                    name = s.name,
                    sourceUris = JsonStringList.encode(s.sourceUris),
                    destPath = s.destPath,
                    mirrorMode = s.mirrorMode,
                    autoSync = s.autoSync,
                    excludePatterns = JsonStringList.encode(s.excludePatterns)
                )
            }
            repository.upsert(profile)
            _state.update { it.copy(saved = true) }
        }
    }

    fun delete() {
        val id = _state.value.id ?: return
        viewModelScope.launch {
            repository.deleteById(id)
            _state.update { it.copy(saved = true) }
        }
    }
}
