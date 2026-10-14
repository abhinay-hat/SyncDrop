package com.syncdrop.android.data.model

import com.google.common.truth.Truth.assertThat
import org.junit.Test

class SyncProfileTest {

    @Test
    fun `default id is a non-blank uuid`() {
        val p = SyncProfile(name = "n", sourceUris = "[]", destPath = "/x")
        assertThat(p.id).isNotEmpty()
        assertThat(p.id.length).isEqualTo(36)
    }

    @Test
    fun `two profiles get distinct default ids`() {
        val a = SyncProfile(name = "a", sourceUris = "[]", destPath = "/x")
        val b = SyncProfile(name = "b", sourceUris = "[]", destPath = "/x")
        assertThat(a.id).isNotEqualTo(b.id)
    }

    @Test
    fun `default exclude patterns include nomedia and thumbnails`() {
        val p = SyncProfile(name = "n", sourceUris = "[]", destPath = "/x")
        assertThat(p.excludePatterns).contains(".nomedia")
        assertThat(p.excludePatterns).contains(".thumbnails")
    }
}
