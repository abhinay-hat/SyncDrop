package com.syncdrop.android.data.repository

import org.json.JSONArray

/**
 * Pure helpers to (de)serialize a list of strings to/from a JSON array string.
 * Used for SyncProfile.sourceUris and SyncProfile.excludePatterns.
 */
object JsonStringList {

    fun encode(values: List<String>): String {
        val arr = JSONArray()
        values.forEach { arr.put(it) }
        return arr.toString()
    }

    fun decode(json: String): List<String> {
        if (json.isBlank()) return emptyList()
        return try {
            val arr = JSONArray(json)
            buildList {
                for (i in 0 until arr.length()) {
                    add(arr.getString(i))
                }
            }
        } catch (e: Exception) {
            emptyList()
        }
    }
}
