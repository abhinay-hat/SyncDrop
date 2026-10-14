package com.syncdrop.android.ui.history

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Error
import androidx.compose.material3.Card
import androidx.compose.material3.ExperimentalMaterial3Api
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
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.syncdrop.android.ui.Formatters
import com.syncdrop.android.ui.theme.ErrorRed
import com.syncdrop.android.ui.theme.SuccessGreen

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun HistoryScreen(
    onBack: () -> Unit,
    viewModel: HistoryViewModel = hiltViewModel()
) {
    val state by viewModel.uiState.collectAsStateWithLifecycle()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("History") },
                actions = {
                    OutlinedButton(onClick = { viewModel.clearHistory() }) { Text("Clear") }
                }
            )
        }
    ) { padding ->
        LazyColumn(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp)
        ) {
            items(state.rows, key = { it.record.id }) { row ->
                Card(modifier = Modifier.fillMaxWidth()) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.SpaceBetween
                    ) {
                        Column(modifier = Modifier.weight(1f)) {
                            Text(row.profileName, style = MaterialTheme.typography.titleLarge)
                            Text(
                                Formatters.formatDate(row.record.startedAt),
                                style = MaterialTheme.typography.labelSmall
                            )
                            Text(
                                "${row.record.filesCopied} files · " +
                                    "${Formatters.formatBytes(row.record.bytesTransferred)} · " +
                                    Formatters.formatDuration(row.record.startedAt, row.record.finishedAt),
                                style = MaterialTheme.typography.bodyLarge
                            )
                            row.record.errorMessage?.let { msg ->
                                Text(msg, color = ErrorRed, style = MaterialTheme.typography.labelSmall)
                            }
                        }
                        if (row.record.succeeded) {
                            Icon(Icons.Filled.CheckCircle, contentDescription = "Success", tint = SuccessGreen)
                        } else {
                            Icon(Icons.Filled.Error, contentDescription = "Failed", tint = ErrorRed)
                        }
                    }
                }
            }
        }
    }
}
