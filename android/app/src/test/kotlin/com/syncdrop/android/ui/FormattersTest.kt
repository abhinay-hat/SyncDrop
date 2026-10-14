package com.syncdrop.android.ui

import com.google.common.truth.Truth.assertThat
import org.junit.Test

class FormattersTest {

    @Test
    fun `bytes under 1024 shown as bytes`() {
        assertThat(Formatters.formatBytes(512)).isEqualTo("512 B")
    }

    @Test
    fun `kilobytes formatted`() {
        assertThat(Formatters.formatBytes(2048)).isEqualTo("2.0 KB")
    }

    @Test
    fun `megabytes formatted`() {
        assertThat(Formatters.formatBytes(5L * 1024 * 1024)).isEqualTo("5.0 MB")
    }

    @Test
    fun `duration under a minute`() {
        assertThat(Formatters.formatDuration(0, 45_000)).isEqualTo("45s")
    }

    @Test
    fun `duration over a minute`() {
        assertThat(Formatters.formatDuration(0, 95_000)).isEqualTo("1m 35s")
    }

    @Test
    fun `zero epoch date shows dash`() {
        assertThat(Formatters.formatDate(0)).isEqualTo("—")
    }
}
