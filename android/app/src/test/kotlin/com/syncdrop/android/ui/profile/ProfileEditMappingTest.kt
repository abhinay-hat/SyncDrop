package com.syncdrop.android.ui.profile

import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.repository.JsonStringList
import com.google.common.truth.Truth.assertThat
import org.junit.Test

/**
 * Verifies the pure mapping between ProfileEditUiState fields and the persisted
 * SyncProfile JSON columns. (The ViewModel save() uses these same transforms.)
 */
class ProfileEditMappingTest {

    @Test
    fun `ui source list maps to json column and back`() {
        val uris = listOf("content://a", "content://b")
        val encoded = JsonStringList.encode(uris)
        val profile = SyncProfile(name = "p", sourceUris = encoded, destPath = "/x")
        assertThat(JsonStringList.decode(profile.sourceUris)).containsExactlyElementsIn(uris).inOrder()
    }

    @Test
    fun `exclude patterns map to json column and back`() {
        val patterns = listOf(".nomedia", "*.tmp")
        val encoded = JsonStringList.encode(patterns)
        val profile = SyncProfile(name = "p", sourceUris = "[]", destPath = "/x", excludePatterns = encoded)
        assertThat(JsonStringList.decode(profile.excludePatterns)).containsExactlyElementsIn(patterns).inOrder()
    }
}
