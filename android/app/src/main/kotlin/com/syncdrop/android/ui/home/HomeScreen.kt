package com.syncdrop.android.ui.home

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.History
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.repository.JsonStringList
import com.syncdrop.android.ui.Formatters
import com.syncdrop.android.ui.theme.SuccessGreen
import java.util.UUID

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun HomeScreen(
    onAddProfile: () -> Unit,
    onEditProfile: (String) -> Unit,
    onOpenHistory: () -> Unit,
    onSyncStarted: (UUID) -> Unit,
    viewModel: HomeViewModel = hiltViewModel()
) {
    val state by viewModel.uiState.collectAsStateWithLifecycle()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("SyncDrop") },
                actions = {
                    OutlinedButton(onClick = onOpenHistory) {
                        Icon(Icons.Filled.History, contentDescription = "History")
                        Text("  History")
                    }
                }
            )
        },
        floatingActionButton = {
            FloatingActionButton(onClick = onAddProfile) {
                Icon(Icons.Filled.Add, contentDescription = "Add Profile")
            }
        }
    ) { padding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding)
        ) {
            UsbBanner(connected = state.usbConnected, label = state.usbLabel)

            LazyColumn(
                modifier = Modifier.fillMaxSize(),
                contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp)
            ) {
                items(state.profiles, key = { it.id }) { profile ->
                    val isSafOrDrive = profile.destPath.startsWith("content://")
                    ProfileCard(
                        profile = profile,
                        lastSyncMillis = state.latestByProfile[profile.id]?.startedAt ?: 0L,
                        syncEnabled = state.usbConnected || isSafOrDrive,
                        onClick = { onEditProfile(profile.id) },
                        onLongPress = { viewModel.deleteProfile(profile) },
                        onSyncNow = { onSyncStarted(viewModel.syncNow(profile)) }
                    )
                }
            }
        }
    }
}

@Composable
private fun UsbBanner(connected: Boolean, label: String?) {
    val bg = if (connected) SuccessGreen else MaterialTheme.colorScheme.surfaceVariant
    val text = if (connected) "SSD connected${label?.let { " — $it" } ?: ""}" else "No SSD connected"
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .background(bg)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        Text(
            text = text,
            color = if (connected) Color.White else MaterialTheme.colorScheme.onSurfaceVariant,
            style = MaterialTheme.typography.bodyLarge
        )
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun ProfileCard(
    profile: SyncProfile,
    lastSyncMillis: Long,
    syncEnabled: Boolean,
    onClick: () -> Unit,
    onLongPress: () -> Unit,
    onSyncNow: () -> Unit
) {
    val sourceCount = JsonStringList.decode(profile.sourceUris).size
    val destDisplay = if (profile.destPath.startsWith("content://")) {
        val decoded = android.net.Uri.decode(profile.destPath)
        "Google Drive / SAF: " + decoded.substringAfterLast('/').ifBlank { "Folder" }
    } else {
        profile.destPath
    }
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .combinedClickable(onClick = onClick, onLongClick = onLongPress)
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(text = profile.name, style = MaterialTheme.typography.titleLarge)
            Text(
                text = "$sourceCount folder(s) → $destDisplay",
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
            Text(
                text = "Last sync: ${Formatters.formatDate(lastSyncMillis)}",
                style = MaterialTheme.typography.labelSmall
            )
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = 8.dp),
                horizontalArrangement = Arrangement.End
            ) {
                OutlinedButton(onClick = onSyncNow, enabled = syncEnabled) {
                    Text("Sync Now")
                }
            }
        }
    }
}
