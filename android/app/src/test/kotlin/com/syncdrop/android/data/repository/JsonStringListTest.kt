package com.syncdrop.android.data.repository

import com.google.common.truth.Truth.assertThat
import org.junit.Test

class JsonStringListTest {

    @Test
    fun `encode then decode round-trips`() {
        val input = listOf("content://a/1", "content://b/2", ".nomedia")
        val json = JsonStringList.encode(input)
        val out = JsonStringList.decode(json)
        assertThat(out).containsExactlyElementsIn(input).inOrder()
    }

    @Test
    fun `decode empty string returns empty list`() {
        assertThat(JsonStringList.decode("")).isEmpty()
    }

    @Test
    fun `decode malformed json returns empty list`() {
        assertThat(JsonStringList.decode("not json")).isEmpty()
    }

    @Test
    fun `decode default exclude patterns`() {
        val out = JsonStringList.decode("[\".nomedia\",\".thumbnails\"]")
        assertThat(out).containsExactly(".nomedia", ".thumbnails")
    }

    @Test
    fun `encode empty list is empty json array`() {
        assertThat(JsonStringList.encode(emptyList())).isEqualTo("[]")
    }
}
