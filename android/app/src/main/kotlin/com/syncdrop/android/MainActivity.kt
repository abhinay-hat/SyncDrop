package com.syncdrop.android

import android.Manifest
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.syncdrop.android.ui.history.HistoryScreen
import com.syncdrop.android.ui.home.HomeScreen
import com.syncdrop.android.ui.profile.ProfileEditScreen
import com.syncdrop.android.ui.progress.SyncProgressScreen
import com.syncdrop.android.ui.theme.SyncDropTheme
import dagger.hilt.android.AndroidEntryPoint
import java.util.UUID

@AndroidEntryPoint
class MainActivity : ComponentActivity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            SyncDropTheme {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    SyncDropNavHost(::requestNotificationsPermission)
                }
            }
        }
    }

    private val notificationPermissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { /* no-op */ }

    private fun requestNotificationsPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            notificationPermissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }
}

@androidx.compose.runtime.Composable
private fun SyncDropNavHost(requestNotifications: () -> Unit) {
    val navController = rememberNavController()

    LaunchedEffect(Unit) { requestNotifications() }

    NavHost(navController = navController, startDestination = "home") {

        composable("home") {
            HomeScreen(
                onAddProfile = { navController.navigate("profile/new") },
                onEditProfile = { id -> navController.navigate("profile/$id") },
                onOpenHistory = { navController.navigate("history") },
                onSyncStarted = { workId -> navController.navigate("progress/$workId") }
            )
        }

        composable("profile/new") {
            ProfileEditScreen(
                profileId = null,
                onDone = { navController.popBackStack() }
            )
        }

        composable(
            route = "profile/{profileId}",
            arguments = listOf(navArgument("profileId") { type = NavType.StringType })
        ) { backStackEntry ->
            val profileId = backStackEntry.arguments?.getString("profileId")
            // "new" is handled by its own route; guard just in case.
            ProfileEditScreen(
                profileId = if (profileId == "new") null else profileId,
                onDone = { navController.popBackStack() }
            )
        }

        composable(
            route = "progress/{workId}",
            arguments = listOf(navArgument("workId") { type = NavType.StringType })
        ) { backStackEntry ->
            val workId = backStackEntry.arguments?.getString("workId")
            if (workId == null) {
                navController.popBackStack("home", inclusive = false)
            } else {
                SyncProgressScreen(
                    workId = UUID.fromString(workId),
                    onDone = { navController.popBackStack("home", inclusive = false) }
                )
            }
        }

        composable("history") {
            HistoryScreen(onBack = { navController.popBackStack() })
        }
    }
}
