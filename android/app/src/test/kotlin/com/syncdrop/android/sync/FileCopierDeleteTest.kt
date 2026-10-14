package com.syncdrop.android.sync

import com.google.common.truth.Truth.assertThat
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class FileCopierDeleteTest {

    @get:Rule
    val temp = TemporaryFolder()

    @Test
    fun `delete removes an existing file`() {
        val root = temp.newFolder("dest")
        val target = java.io.File(root, "sub/file.txt").apply {
            parentFile?.mkdirs()
            writeText("hello")
        }
        assertThat(target.exists()).isTrue()
        val ok = FileCopier.delete("sub/file.txt", root.absolutePath)
        assertThat(ok).isTrue()
        assertThat(target.exists()).isFalse()
    }

    @Test
    fun `delete on missing file returns true`() {
        val root = temp.newFolder("dest")
        val ok = FileCopier.delete("nope.txt", root.absolutePath)
        assertThat(ok).isTrue()
    }
}
