package com.syncdrop.android.data.repository

import com.syncdrop.android.data.db.SyncProfileDao
import com.syncdrop.android.data.model.SyncProfile
import io.mockk.coVerify
import io.mockk.coEvery
import io.mockk.mockk
import kotlinx.coroutines.test.runTest
import org.junit.Test

class ProfileRepositoryTest {

    private val dao = mockk<SyncProfileDao>(relaxed = true)
    private val repo = ProfileRepository(dao)

    @Test
    fun `upsert delegates to dao insert`() = runTest {
        val p = SyncProfile(name = "n", sourceUris = "[]", destPath = "/x")
        repo.upsert(p)
        coVerify { dao.insert(p) }
    }

    @Test
    fun `getProfile delegates to dao getById`() = runTest {
        val p = SyncProfile(id = "id1", name = "n", sourceUris = "[]", destPath = "/x")
        coEvery { dao.getById("id1") } returns p
        val result = repo.getProfile("id1")
        coVerify { dao.getById("id1") }
        assert(result == p)
    }

    @Test
    fun `deleteById delegates to dao`() = runTest {
        repo.deleteById("id1")
        coVerify { dao.deleteById("id1") }
    }
}
