package com.syncdrop.android.ui.profile

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.InputChip
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.syncdrop.android.R

@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun ProfileEditScreen(
    profileId: String?,
    onDone: () -> Unit,
    viewModel: ProfileEditViewModel = hiltViewModel()
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current

    LaunchedEffect(profileId) { viewModel.load(profileId) }
    LaunchedEffect(state.saved) { if (state.saved) onDone() }

    val folderPicker = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocumentTree()
    ) { uri ->
        if (uri != null) {
            // Persist read permission so the worker can access it later.
            context.contentResolver.takePersistableUriPermission(
                uri,
                android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION
            )
            viewModel.addSource(uri.toString())
        }
    }

    val destFolderPicker = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.OpenDocumentTree()
    ) { uri ->
        if (uri != null) {
            context.contentResolver.takePersistableUriPermission(
                uri,
                android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or
                    android.content.Intent.FLAG_GRANT_WRITE_URI_PERMISSION
            )
            viewModel.onDestPathChange(uri.toString())
        }
    }

    var newPattern by remember { mutableStateOf("") }

    Scaffold(
        topBar = { TopAppBar(title = { Text(if (profileId == null) "New Profile" else "Edit Profile") }) }
    ) { padding ->
        LazyColumn(
            modifier = Modifier
                .fillMaxWidth()
                .padding(padding)
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp)
        ) {
            item {
                OutlinedTextField(
                    value = state.name,
                    onValueChange = viewModel::onNameChange,
                    label = { Text("Profile name") },
                    modifier = Modifier.fillMaxWidth()
                )
            }

            item { Text("Source folders", style = androidx.compose.material3.MaterialTheme.typography.titleLarge) }

            items(state.sourceUris, key = { it }) { uri ->
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    Text(
                        text = displayNameForUri(uri),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f)
                    )
                    OutlinedButton(onClick = { viewModel.removeSource(uri) }) { Text("Remove") }
                }
            }

            item {
                OutlinedButton(onClick = { folderPicker.launch(null) }) { Text("Add Folder") }
            }

            item { Text("Destination", style = androidx.compose.material3.MaterialTheme.typography.titleLarge) }

            item {
                if (state.destPath.startsWith("content://")) {
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(6.dp)
                    ) {
                        Text(
                            text = "Google Drive / Remote: ${displayNameForUri(state.destPath)}",
                            style = androidx.compose.material3.MaterialTheme.typography.bodyLarge,
                            color = androidx.compose.material3.MaterialTheme.colorScheme.primary
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            OutlinedButton(onClick = { destFolderPicker.launch(null) }) {
                                Text("Change Folder / Drive")
                            }
                            OutlinedButton(onClick = { viewModel.onDestPathChange("") }) {
                                Text("Clear")
                            }
                        }
                    }
                } else {
                    Column(
                        modifier = Modifier.fillMaxWidth(),
                        verticalArrangement = Arrangement.spacedBy(6.dp)
                    ) {
                        OutlinedTextField(
                            value = state.destPath,
                            onValueChange = viewModel::onDestPathChange,
                            label = { Text("Destination path or Google Drive") },
                            modifier = Modifier.fillMaxWidth()
                        )
                        OutlinedButton(onClick = { destFolderPicker.launch(null) }) {
                            Text("Choose Folder or Google Drive…")
                        }
                    }
                }
            }

            item {
                Text(
                    text = if (state.destPath.startsWith("content://"))
                        "Using Storage Access Framework (SAF / Google Drive). Files will sync directly to this folder."
                    else
                        stringResource(R.string.storage_note),
                    style = androidx.compose.material3.MaterialTheme.typography.labelSmall,
                    color = androidx.compose.material3.MaterialTheme.colorScheme.onSurfaceVariant
                )
            }

            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    Text("Auto-sync")
                    Switch(checked = state.autoSync, onCheckedChange = viewModel::onAutoSyncToggle)
                }
            }

            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.SpaceBetween
                ) {
                    Text("Mirror mode (delete extras on SSD)")
                    Switch(checked = state.mirrorMode, onCheckedChange = viewModel::onMirrorToggle)
                }
            }

            item { Text("Exclude patterns", style = androidx.compose.material3.MaterialTheme.typography.titleLarge) }

            item {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    state.excludePatterns.forEach { pattern ->
                        InputChip(
                            selected = false,
                            onClick = { viewModel.removeExcludePattern(pattern) },
                            label = { Text(pattern) },
                            trailingIcon = {
                                Icon(Icons.Filled.Close, contentDescription = "Remove $pattern")
                            }
                        )
                    }
                }
            }

            item {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp)
                ) {
                    OutlinedTextField(
                        value = newPattern,
                        onValueChange = { newPattern = it },
                        label = { Text("Add pattern e.g. *.tmp") },
                        modifier = Modifier.weight(1f)
                    )
                    OutlinedButton(onClick = {
                        viewModel.addExcludePattern(newPattern)
                        newPattern = ""
                    }) { Text("Add") }
                }
            }

            item {
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(top = 12.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp)
                ) {
                    Button(
                        onClick = viewModel::save,
                        enabled = state.name.isNotBlank()
                    ) { Text("Save") }

                    if (profileId != null) {
                        OutlinedButton(onClick = viewModel::delete) { Text("Delete") }
                    }
                }
            }
        }
    }
}

/** Derives a human-friendly display name from a SAF tree URI string. */
private fun displayNameForUri(uri: String): String {
    val decoded = android.net.Uri.decode(uri)
    return decoded.substringAfterLast('/').ifBlank { decoded }
}
