package com.syncdrop.android.sync

import android.net.Uri
import com.google.common.truth.Truth.assertThat
import io.mockk.mockk
import org.junit.Test

class FileComparerTest {

    private val fakeUri = mockk<Uri>(relaxed = true)

    private fun src(path: String, size: Long, mod: Long) =
        FileEntry(uri = fakeUri, name = path.substringAfterLast('/'), size = size, lastModified = mod, relativePath = path)

    private fun dst(path: String, size: Long, mod: Long) =
        DestEntry(relativePath = path, size = size, lastModified = mod)

    @Test
    fun `new file is copied`() {
        val source = mapOf("a/x.txt" to src("a/x.txt", 10, 100))
        val dest = emptyMap<String, DestEntry>()
        val diff = FileComparer.diff(source, dest, mirrorMode = false)
        assertThat(diff.toCopy.map { it.relativePath }).containsExactly("a/x.txt")
        assertThat(diff.toDelete).isEmpty()
    }

    @Test
    fun `unchanged file is skipped`() {
        val source = mapOf("a/x.txt" to src("a/x.txt", 10, 100))
        val dest = mapOf("a/x.txt" to dst("a/x.txt", 10, 100))
        val diff = FileComparer.diff(source, dest, mirrorMode = false)
        assertThat(diff.toCopy).isEmpty()
    }

    @Test
    fun `newer source is copied`() {
        val source = mapOf("a/x.txt" to src("a/x.txt", 10, 200))
        val dest = mapOf("a/x.txt" to dst("a/x.txt", 10, 100))
        val diff = FileComparer.diff(source, dest, mirrorMode = false)
        assertThat(diff.toCopy.map { it.relativePath }).containsExactly("a/x.txt")
    }

    @Test
    fun `size mismatch is copied`() {
        val source = mapOf("a/x.txt" to src("a/x.txt", 20, 100))
        val dest = mapOf("a/x.txt" to dst("a/x.txt", 10, 100))
        val diff = FileComparer.diff(source, dest, mirrorMode = false)
        assertThat(diff.toCopy.map { it.relativePath }).containsExactly("a/x.txt")
    }

    @Test
    fun `mirror mode deletes extra dest files`() {
        val source = mapOf("a/x.txt" to src("a/x.txt", 10, 100))
        val dest = mapOf(
            "a/x.txt" to dst("a/x.txt", 10, 100),
            "a/orphan.txt" to dst("a/orphan.txt", 5, 50)
        )
        val diff = FileComparer.diff(source, dest, mirrorMode = true)
        assertThat(diff.toDelete).containsExactly("a/orphan.txt")
    }

    @Test
    fun `non-mirror mode never deletes`() {
        val source = mapOf("a/x.txt" to src("a/x.txt", 10, 100))
        val dest = mapOf("a/orphan.txt" to dst("a/orphan.txt", 5, 50))
        val diff = FileComparer.diff(source, dest, mirrorMode = false)
        assertThat(diff.toDelete).isEmpty()
    }

    @Test
    fun `exact name exclusion matches`() {
        assertThat(FileComparer.isExcluded(".nomedia", listOf(".nomedia"))).isTrue()
        assertThat(FileComparer.isExcluded("photo.jpg", listOf(".nomedia"))).isFalse()
    }

    @Test
    fun `glob extension exclusion matches`() {
        assertThat(FileComparer.isExcluded("a.tmp", listOf("*.tmp"))).isTrue()
        assertThat(FileComparer.isExcluded("a.TMP", listOf("*.tmp"))).isTrue()
        assertThat(FileComparer.isExcluded("a.txt", listOf("*.tmp"))).isFalse()
    }

    @Test
    fun `empty pattern is ignored`() {
        assertThat(FileComparer.isExcluded("anything", listOf(""))).isFalse()
    }
}
