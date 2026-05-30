# SyncDrop Android App — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build SyncDrop for Android — USB OTG SSD detection, folder selection, and one-way file sync from phone to SSD.

**Architecture:** Single-activity Compose app. Room for persistence, WorkManager for background sync, Hilt for DI. USB detection via BroadcastReceiver. File access via Storage Access Framework (SAF DocumentFile API). Sync logic is pure Kotlin in the domain layer.

**Tech Stack:** Kotlin, Jetpack Compose, Room, WorkManager, Hilt, Coroutines, SAF

---

## Conventions & Notes (read before starting)

- **Working directory:** All paths are under `~/SyncDrop/android/`. Create that directory in Task 1.
- **Annotation processor:** This plan uses **KSP** consistently for both Room and Hilt (cleanest for target SDK 34 / 2026 toolchains). Do not mix kapt and KSP.
- **Application class:** A `@HiltAndroidApp` `SyncDropApplication` is mandatory (not in the original file tree). It also implements `Configuration.Provider` to supply the `HiltWorkerFactory` to WorkManager, and the default WorkManager auto-initializer is disabled in the manifest. This is the single most fragile integration — Tasks 1, 4, and 13 must be consistent.
- **Known limitation (flagged, not redesigned):** The spec writes to the USB destination via a `java.io.File` path (`/storage/XXXX-XXXX/...`) using `FileOutputStream`. On target SDK 34 with scoped storage, direct `File` writes to a removable volume are generally blocked for normal apps; a production build would need the user to grant a SAF tree on the SSD and write the destination via `DocumentFile` too. **We follow the spec as written (File-based dest)** so the sync layer stays as specified; the limitation is documented in `strings.xml` and surfaced in the UI as guidance. Future work: SAF-based destination.
- **Testing reality:** Genuinely unit-testable pure logic — the diff algorithm (over in-memory maps), exclude-pattern matching, relative-path computation, and URI↔JSON (de)serialization — gets JUnit4 + MockK tests. Anything touching `DocumentFile`/`Uri`/USB/Compose gets documented manual verification steps.
- **Each task ends with `git add` + `git commit`.** Commits are scoped to the task. The repo already exists at `~/SyncDrop/` (a git repo). Do not `git init`. Run git commands from `/Users/padidamabhinay/SyncDrop`.
- **Module path note:** All `android/` paths below are relative to `~/SyncDrop/`. When the plan says `android/app/build.gradle.kts`, the absolute path is `/Users/padidamabhinay/SyncDrop/android/app/build.gradle.kts`.

---

## Task 1 — Gradle scaffolding, manifest, and base resources

**Goal:** Create a buildable empty Android Gradle project with all dependencies declared.

### `android/settings.gradle.kts`
```kotlin
pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "SyncDrop"
include(":app")
```

### `android/gradle.properties`
```properties
org.gradle.jvmargs=-Xmx2048m -Dfile.encoding=UTF-8
org.gradle.caching=true
org.gradle.configuration-cache=false
android.useAndroidX=true
kotlin.code.style=official
android.nonTransitiveRClass=true
```

### `android/build.gradle.kts` (root)
```kotlin
plugins {
    id("com.android.application") version "8.5.2" apply false
    id("org.jetbrains.kotlin.android") version "2.0.20" apply false
    id("com.google.devtools.ksp") version "2.0.20-1.0.25" apply false
    id("com.google.dagger.hilt.android") version "2.52" apply false
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.20" apply false
}
```

### `android/app/build.gradle.kts`
```kotlin
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("com.google.devtools.ksp")
    id("com.google.dagger.hilt.android")
}

android {
    namespace = "com.syncdrop.android"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.syncdrop.android"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
    }

    sourceSets {
        getByName("main") {
            kotlin.srcDirs("src/main/kotlin")
        }
        getByName("test") {
            kotlin.srcDirs("src/test/kotlin")
        }
        getByName("androidTest") {
            kotlin.srcDirs("src/androidTest/kotlin")
        }
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.09.02")
    implementation(composeBom)
    androidTestImplementation(composeBom)

    // Core
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.6")
    implementation("androidx.activity:activity-compose:1.9.2")

    // Compose
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")

    // Navigation
    implementation("androidx.navigation:navigation-compose:2.8.0")

    // Lifecycle / ViewModel for Compose
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.6")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.6")

    // Room
    implementation("androidx.room:room-runtime:2.6.1")
    implementation("androidx.room:room-ktx:2.6.1")
    ksp("androidx.room:room-compiler:2.6.1")

    // WorkManager
    implementation("androidx.work:work-runtime-ktx:2.9.1")

    // Hilt
    implementation("com.google.dagger:hilt-android:2.52")
    ksp("com.google.dagger:hilt-compiler:2.52")
    implementation("androidx.hilt:hilt-navigation-compose:1.2.0")
    implementation("androidx.hilt:hilt-work:1.2.0")
    ksp("androidx.hilt:hilt-compiler:1.2.0")

    // DocumentFile (SAF)
    implementation("androidx.documentfile:documentfile:1.0.1")

    // Coroutines
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")

    // Unit tests
    testImplementation("junit:junit:4.13.2")
    testImplementation("io.mockk:mockk:1.13.12")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.8.1")
    testImplementation("com.google.truth:truth:1.4.4")

    // Instrumented tests
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.6.1")
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
}
```

### `android/app/proguard-rules.pro`
```proguard
# Keep Room generated code
-keep class * extends androidx.room.RoomDatabase
-dontwarn androidx.room.paging.**
```

### `android/app/src/main/AndroidManifest.xml`
```xml
<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">

    <uses-feature
        android:name="android.hardware.usb.host"
        android:required="false" />

    <uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
    <uses-permission android:name="android.permission.FOREGROUND_SERVICE_DATA_SYNC" />
    <uses-permission android:name="android.permission.POST_NOTIFICATIONS" />

    <application
        android:name=".SyncDropApplication"
        android:allowBackup="true"
        android:icon="@android:drawable/ic_menu_save"
        android:label="@string/app_name"
        android:supportsRtl="true"
        android:theme="@style/Theme.SyncDrop">

        <activity
            android:name=".MainActivity"
            android:exported="true"
            android:label="@string/app_name"
            android:theme="@style/Theme.SyncDrop">

            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>

            <intent-filter>
                <action android:name="android.hardware.usb.action.USB_DEVICE_ATTACHED" />
            </intent-filter>

            <meta-data
                android:name="android.hardware.usb.action.USB_DEVICE_ATTACHED"
                android:resource="@xml/usb_device_filter" />
        </activity>

        <receiver
            android:name=".usb.UsbReceiver"
            android:exported="true">
            <intent-filter>
                <action android:name="android.hardware.usb.action.USB_DEVICE_ATTACHED" />
                <action android:name="android.hardware.usb.action.USB_DEVICE_DETACHED" />
            </intent-filter>
        </receiver>

        <!-- Disable WorkManager default initializer; we provide a custom
             Configuration via SyncDropApplication (Configuration.Provider). -->
        <provider
            android:name="androidx.startup.InitializationProvider"
            android:authorities="${applicationId}.androidx-startup"
            android:exported="false"
            tools:node="merge"
            xmlns:tools="http://schemas.android.com/tools">
            <meta-data
                android:name="androidx.work.WorkManagerInitializer"
                android:value="androidx.startup"
                tools:node="remove"
                xmlns:tools="http://schemas.android.com/tools" />
        </provider>

    </application>
</manifest>
```

### `android/app/src/main/res/values/strings.xml`
```xml
<?xml version="1.0" encoding="utf-8"?>
<resources>
    <string name="app_name">SyncDrop</string>
    <string name="usb_connected">SSD connected</string>
    <string name="usb_not_connected">No SSD connected</string>
    <string name="add_profile">Add Profile</string>
    <string name="sync_now">Sync Now</string>
    <string name="add_folder">Add Folder</string>
    <string name="auto_sync">Auto-sync</string>
    <string name="mirror_mode">Mirror mode</string>
    <string name="save">Save</string>
    <string name="delete">Delete</string>
    <string name="cancel">Cancel</string>
    <string name="clear_history">Clear History</string>
    <string name="history">History</string>
    <string name="profile_name">Profile name</string>
    <string name="dest_path">Destination path on SSD</string>
    <string name="exclude_patterns">Exclude patterns</string>
    <string name="sync_channel_name">Sync progress</string>
    <string name="sync_channel_desc">Shows progress while syncing to your SSD</string>
    <string name="storage_note">Note: writing to a removable USB volume requires that Android has mounted it at a readable path. If the destination is not writable, grant storage access and re-plug the SSD.</string>
</resources>
```

### `android/app/src/main/res/values/themes.xml`
```xml
<?xml version="1.0" encoding="utf-8"?>
<resources>
    <style name="Theme.SyncDrop" parent="android:Theme.Material.Light.NoActionBar" />
</resources>
```

### `android/app/src/main/res/xml/file_paths.xml`
```xml
<?xml version="1.0" encoding="utf-8"?>
<paths>
    <external-path name="external_files" path="." />
    <files-path name="internal_files" path="." />
</paths>
```

### `android/app/src/main/res/xml/usb_device_filter.xml`
```xml
<?xml version="1.0" encoding="utf-8"?>
<resources>
    <!-- USB Mass Storage class 0x08 -->
    <usb-device class="8" />
</resources>
```

### `android/.gitignore`
```gitignore
.gradle/
build/
local.properties
*.iml
.idea/
.kotlin/
captures/
```

### Verification (manual — no buildable code yet beyond config)
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew help` — requires the Gradle wrapper. If the wrapper is absent, generate it with a system Gradle: `gradle wrapper --gradle-version 8.9` (run once, commit the wrapper). The project will not fully assemble until later tasks add source files; `help` confirms the plugin/dependency graph resolves.
- [ ] Confirm `android/app/src/main/kotlin/com/syncdrop/android/` directory exists (create it now even if empty): `mkdir -p android/app/src/main/kotlin/com/syncdrop/android`.

### Commit
- [ ] `git add android/settings.gradle.kts android/gradle.properties android/build.gradle.kts android/app/build.gradle.kts android/app/proguard-rules.pro android/app/src/main/AndroidManifest.xml android/app/src/main/res android/.gitignore android/gradle android/gradlew android/gradlew.bat`
- [ ] `git commit -m "Task 1: Gradle scaffolding, manifest, and base resources"`

---

## Task 2 — Theme (Color, Type, Theme)

**Goal:** Material 3 Compose theme.

### `android/app/src/main/kotlin/com/syncdrop/android/ui/theme/Color.kt`
```kotlin
package com.syncdrop.android.ui.theme

import androidx.compose.ui.graphics.Color

val Purple80 = Color(0xFFD0BCFF)
val PurpleGrey80 = Color(0xFFCCC2DC)
val Pink80 = Color(0xFFEFB8C8)

val Purple40 = Color(0xFF6650A4)
val PurpleGrey40 = Color(0xFF625B71)
val Pink40 = Color(0xFF7D5260)

val SuccessGreen = Color(0xFF2E7D32)
val ErrorRed = Color(0xFFC62828)
```

### `android/app/src/main/kotlin/com/syncdrop/android/ui/theme/Type.kt`
```kotlin
package com.syncdrop.android.ui.theme

import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

val Typography = Typography(
    bodyLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Normal,
        fontSize = 16.sp,
        lineHeight = 24.sp,
        letterSpacing = 0.5.sp
    ),
    titleLarge = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.SemiBold,
        fontSize = 22.sp,
        lineHeight = 28.sp,
        letterSpacing = 0.sp
    ),
    labelSmall = TextStyle(
        fontFamily = FontFamily.Default,
        fontWeight = FontWeight.Medium,
        fontSize = 11.sp,
        lineHeight = 16.sp,
        letterSpacing = 0.5.sp
    )
)
```

### `android/app/src/main/kotlin/com/syncdrop/android/ui/theme/Theme.kt`
```kotlin
package com.syncdrop.android.ui.theme

import android.app.Activity
import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

private val DarkColorScheme = darkColorScheme(
    primary = Purple80,
    secondary = PurpleGrey80,
    tertiary = Pink80
)

private val LightColorScheme = lightColorScheme(
    primary = Purple40,
    secondary = PurpleGrey40,
    tertiary = Pink40
)

@Composable
fun SyncDropTheme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    dynamicColor: Boolean = true,
    content: @Composable () -> Unit
) {
    val colorScheme = when {
        dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val context = LocalContext.current
            if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
        }
        darkTheme -> DarkColorScheme
        else -> LightColorScheme
    }

    MaterialTheme(
        colorScheme = colorScheme,
        typography = Typography,
        content = content
    )
}
```

### Verification
- [ ] No unit tests for theme. Verified visually once `MainActivity` exists (Task 13).

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/ui/theme/`
- [ ] `git commit -m "Task 2: Material 3 Compose theme"`

---

## Task 3 — Data models (Room entities)

**Goal:** `SyncProfile` and `SyncRecord` entities.

### `android/app/src/main/kotlin/com/syncdrop/android/data/model/SyncProfile.kt`
```kotlin
package com.syncdrop.android.data.model

import androidx.room.Entity
import androidx.room.PrimaryKey
import java.util.UUID

@Entity(tableName = "sync_profiles")
data class SyncProfile(
    @PrimaryKey val id: String = UUID.randomUUID().toString(),
    val name: String,
    val sourceUris: String,        // JSON array of SAF URI strings
    val destPath: String,          // path on USB volume e.g. "/storage/XXXX-XXXX/SyncDrop"
    val mirrorMode: Boolean = false,
    val autoSync: Boolean = false,
    val excludePatterns: String = "[\".nomedia\",\".thumbnails\"]", // JSON array
    val createdAt: Long = System.currentTimeMillis()
)
```

### `android/app/src/main/kotlin/com/syncdrop/android/data/model/SyncRecord.kt`
```kotlin
package com.syncdrop.android.data.model

import androidx.room.Entity
import androidx.room.PrimaryKey
import java.util.UUID

@Entity(tableName = "sync_records")
data class SyncRecord(
    @PrimaryKey val id: String = UUID.randomUUID().toString(),
    val profileId: String,
    val startedAt: Long,
    val finishedAt: Long,
    val filesCopied: Int,
    val bytesTransferred: Long,
    val succeeded: Boolean,
    val errorMessage: String? = null
)
```

### Test: `android/app/src/test/kotlin/com/syncdrop/android/data/model/SyncProfileTest.kt`
```kotlin
package com.syncdrop.android.data.model

import com.google.common.truth.Truth.assertThat
import org.junit.Test

class SyncProfileTest {

    @Test
    fun `default id is a non-blank uuid`() {
        val p = SyncProfile(name = "n", sourceUris = "[]", destPath = "/x")
        assertThat(p.id).isNotEmpty()
        assertThat(p.id.length).isEqualTo(36)
    }

    @Test
    fun `two profiles get distinct default ids`() {
        val a = SyncProfile(name = "a", sourceUris = "[]", destPath = "/x")
        val b = SyncProfile(name = "b", sourceUris = "[]", destPath = "/x")
        assertThat(a.id).isNotEqualTo(b.id)
    }

    @Test
    fun `default exclude patterns include nomedia and thumbnails`() {
        val p = SyncProfile(name = "n", sourceUris = "[]", destPath = "/x")
        assertThat(p.excludePatterns).contains(".nomedia")
        assertThat(p.excludePatterns).contains(".thumbnails")
    }
}
```

### Verification
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:testDebugUnitTest --tests "com.syncdrop.android.data.model.SyncProfileTest"`

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/data/model/ android/app/src/test/kotlin/com/syncdrop/android/data/model/`
- [ ] `git commit -m "Task 3: Room entities SyncProfile and SyncRecord"`

---

## Task 4 — DAOs, Database, and Hilt module

**Goal:** Room DAOs, the `AppDatabase`, and the Hilt `AppModule` that provides them.

### `android/app/src/main/kotlin/com/syncdrop/android/data/db/SyncProfileDao.kt`
```kotlin
package com.syncdrop.android.data.db

import androidx.room.Dao
import androidx.room.Delete
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import androidx.room.Update
import com.syncdrop.android.data.model.SyncProfile
import kotlinx.coroutines.flow.Flow

@Dao
interface SyncProfileDao {

    @Query("SELECT * FROM sync_profiles ORDER BY createdAt DESC")
    fun observeAll(): Flow<List<SyncProfile>>

    @Query("SELECT * FROM sync_profiles WHERE id = :id")
    suspend fun getById(id: String): SyncProfile?

    @Query("SELECT * FROM sync_profiles WHERE autoSync = 1")
    suspend fun getAutoSyncProfiles(): List<SyncProfile>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(profile: SyncProfile)

    @Update
    suspend fun update(profile: SyncProfile)

    @Delete
    suspend fun delete(profile: SyncProfile)

    @Query("DELETE FROM sync_profiles WHERE id = :id")
    suspend fun deleteById(id: String)
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/data/db/SyncRecordDao.kt`
```kotlin
package com.syncdrop.android.data.db

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import com.syncdrop.android.data.model.SyncRecord
import kotlinx.coroutines.flow.Flow

@Dao
interface SyncRecordDao {

    @Query("SELECT * FROM sync_records ORDER BY startedAt DESC")
    fun observeAll(): Flow<List<SyncRecord>>

    @Query("SELECT * FROM sync_records WHERE profileId = :profileId ORDER BY startedAt DESC LIMIT 1")
    suspend fun getLatestForProfile(profileId: String): SyncRecord?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(record: SyncRecord)

    @Query("DELETE FROM sync_records")
    suspend fun clearAll()
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/data/db/AppDatabase.kt`
```kotlin
package com.syncdrop.android.data.db

import androidx.room.Database
import androidx.room.RoomDatabase
import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.model.SyncRecord

@Database(
    entities = [SyncProfile::class, SyncRecord::class],
    version = 1,
    exportSchema = false
)
abstract class AppDatabase : RoomDatabase() {
    abstract fun syncProfileDao(): SyncProfileDao
    abstract fun syncRecordDao(): SyncRecordDao

    companion object {
        const val DB_NAME = "syncdrop.db"
    }
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/di/AppModule.kt`
```kotlin
package com.syncdrop.android.di

import android.content.Context
import androidx.room.Room
import com.syncdrop.android.data.db.AppDatabase
import com.syncdrop.android.data.db.SyncProfileDao
import com.syncdrop.android.data.db.SyncRecordDao
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
object AppModule {

    @Provides
    @Singleton
    fun provideDatabase(@ApplicationContext context: Context): AppDatabase =
        Room.databaseBuilder(context, AppDatabase::class.java, AppDatabase.DB_NAME)
            .fallbackToDestructiveMigration()
            .build()

    @Provides
    fun provideProfileDao(db: AppDatabase): SyncProfileDao = db.syncProfileDao()

    @Provides
    fun provideRecordDao(db: AppDatabase): SyncRecordDao = db.syncRecordDao()
}
```

### Verification
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:compileDebugKotlin` — confirms Room + Hilt KSP processors generate code without error. (Full assembly still needs the Application class from Task 5.)

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/data/db/ android/app/src/main/kotlin/com/syncdrop/android/di/`
- [ ] `git commit -m "Task 4: Room DAOs, AppDatabase, and Hilt AppModule"`

---

## Task 5 — Application class (Hilt + WorkManager Configuration.Provider)

**Goal:** `@HiltAndroidApp` Application that wires `HiltWorkerFactory` into WorkManager and creates the notification channel.

### `android/app/src/main/kotlin/com/syncdrop/android/SyncDropApplication.kt`
```kotlin
package com.syncdrop.android

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import android.os.Build
import androidx.hilt.work.HiltWorkerFactory
import androidx.work.Configuration
import dagger.hilt.android.HiltAndroidApp
import javax.inject.Inject

@HiltAndroidApp
class SyncDropApplication : Application(), Configuration.Provider {

    @Inject
    lateinit var workerFactory: HiltWorkerFactory

    override val workManagerConfiguration: Configuration
        get() = Configuration.Builder()
            .setWorkerFactory(workerFactory)
            .build()

    override fun onCreate() {
        super.onCreate()
        createSyncNotificationChannel()
    }

    private fun createSyncNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                SYNC_CHANNEL_ID,
                getString(R.string.sync_channel_name),
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = getString(R.string.sync_channel_desc)
            }
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(channel)
        }
    }

    companion object {
        const val SYNC_CHANNEL_ID = "syncdrop_sync_progress"
    }
}
```

### Verification
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:assembleDebug` — should now compile and package; Hilt + WorkManager factory wiring resolves.

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/SyncDropApplication.kt`
- [ ] `git commit -m "Task 5: Hilt Application with WorkManager Configuration.Provider"`

---

## Task 6 — Repositories

**Goal:** `ProfileRepository` and `HistoryRepository` over the DAOs, plus URI↔JSON serialization helpers (pure, unit-tested).

### `android/app/src/main/kotlin/com/syncdrop/android/data/repository/JsonStringList.kt`
```kotlin
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
```

### `android/app/src/main/kotlin/com/syncdrop/android/data/repository/ProfileRepository.kt`
```kotlin
package com.syncdrop.android.data.repository

import com.syncdrop.android.data.db.SyncProfileDao
import com.syncdrop.android.data.model.SyncProfile
import kotlinx.coroutines.flow.Flow
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class ProfileRepository @Inject constructor(
    private val dao: SyncProfileDao
) {
    fun observeProfiles(): Flow<List<SyncProfile>> = dao.observeAll()

    suspend fun getProfile(id: String): SyncProfile? = dao.getById(id)

    suspend fun getAutoSyncProfiles(): List<SyncProfile> = dao.getAutoSyncProfiles()

    suspend fun upsert(profile: SyncProfile) = dao.insert(profile)

    suspend fun delete(profile: SyncProfile) = dao.delete(profile)

    suspend fun deleteById(id: String) = dao.deleteById(id)
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/data/repository/HistoryRepository.kt`
```kotlin
package com.syncdrop.android.data.repository

import com.syncdrop.android.data.db.SyncRecordDao
import com.syncdrop.android.data.model.SyncRecord
import kotlinx.coroutines.flow.Flow
import javax.inject.Inject
import javax.inject.Singleton

@Singleton
class HistoryRepository @Inject constructor(
    private val dao: SyncRecordDao
) {
    fun observeRecords(): Flow<List<SyncRecord>> = dao.observeAll()

    suspend fun latestForProfile(profileId: String): SyncRecord? =
        dao.getLatestForProfile(profileId)

    suspend fun add(record: SyncRecord) = dao.insert(record)

    suspend fun clear() = dao.clearAll()
}
```

### Test: `android/app/src/test/kotlin/com/syncdrop/android/data/repository/JsonStringListTest.kt`
```kotlin
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
```

### Test: `android/app/src/test/kotlin/com/syncdrop/android/data/repository/ProfileRepositoryTest.kt`
```kotlin
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
```

### Verification
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:testDebugUnitTest --tests "com.syncdrop.android.data.repository.*"`

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/data/repository/ android/app/src/test/kotlin/com/syncdrop/android/data/repository/`
- [ ] `git commit -m "Task 6: Profile and History repositories with JSON helpers"`

---

## Task 7 — USB detection (Checker, Receiver, MountHelper)

**Goal:** Detect mass-storage USB devices and resolve the SSD mount path.

### `android/app/src/main/kotlin/com/syncdrop/android/usb/UsbDeviceChecker.kt`
```kotlin
package com.syncdrop.android.usb

import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice

/**
 * Determines whether a connected USB device is a mass-storage device
 * (class 0x08), either at the device level or on any of its interfaces.
 */
object UsbDeviceChecker {

    private const val USB_CLASS_MASS_STORAGE = UsbConstants.USB_CLASS_MASS_STORAGE // 0x08

    fun isMassStorage(device: UsbDevice): Boolean {
        if (device.deviceClass == USB_CLASS_MASS_STORAGE) return true
        for (i in 0 until device.interfaceCount) {
            if (device.getInterface(i).interfaceClass == USB_CLASS_MASS_STORAGE) {
                return true
            }
        }
        return false
    }
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/usb/UsbMountHelper.kt`
```kotlin
package com.syncdrop.android.usb

import android.content.Context
import android.os.Build
import android.os.Environment
import android.os.storage.StorageManager
import android.os.storage.StorageVolume
import java.io.File

/**
 * Resolves the mount path of a removable (USB/SSD) storage volume.
 *
 * Strategy:
 *  1. Prefer StorageManager.storageVolumes — find a removable, mounted, non-primary volume.
 *  2. Fall back to scanning /storage/ for entries that look like a volume UUID
 *     (e.g. "XXXX-XXXX") and are readable.
 */
object UsbMountHelper {

    data class MountedVolume(val path: String, val label: String?)

    fun findMountedVolume(context: Context): MountedVolume? {
        val fromManager = findViaStorageManager(context)
        if (fromManager != null) return fromManager
        return findViaStorageScan()
    }

    fun findMountPath(context: Context): String? = findMountedVolume(context)?.path

    private fun findViaStorageManager(context: Context): MountedVolume? {
        val sm = context.getSystemService(Context.STORAGE_SERVICE) as? StorageManager
            ?: return null
        val volumes: List<StorageVolume> = sm.storageVolumes
        for (vol in volumes) {
            if (vol.isPrimary) continue
            if (!vol.isRemovable) continue
            if (vol.state != Environment.MEDIA_MOUNTED) continue
            val path = resolveVolumePath(vol) ?: continue
            val label = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                vol.mediaStoreVolumeName ?: vol.getDescription(context)
            } else {
                vol.getDescription(context)
            }
            return MountedVolume(path, label)
        }
        return null
    }

    private fun resolveVolumePath(vol: StorageVolume): String? {
        // API 30+: directory is exposed directly.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            vol.directory?.absolutePath?.let { return it }
        }
        // Older APIs / fallback: reflect getPath() (hidden but historically present).
        return try {
            val method = vol.javaClass.getMethod("getPath")
            method.invoke(vol) as? String
        } catch (e: Exception) {
            null
        }
    }

    private fun findViaStorageScan(): MountedVolume? {
        val storageDir = File("/storage")
        val children = storageDir.listFiles() ?: return null
        for (child in children) {
            val name = child.name
            if (name == "self" || name == "emulated") continue
            if (!child.isDirectory) continue
            if (!child.canRead()) continue
            // Volume UUIDs typically look like "XXXX-XXXX".
            return MountedVolume(child.absolutePath, name)
        }
        return null
    }
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/usb/UsbReceiver.kt`
```kotlin
package com.syncdrop.android.usb

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.util.Log

/**
 * Receives USB attach/detach broadcasts and republishes a normalized
 * local broadcast that the UI layer observes (see UsbConnectionState).
 */
class UsbReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            UsbManager.ACTION_USB_DEVICE_ATTACHED -> {
                val device: UsbDevice? = if (android.os.Build.VERSION.SDK_INT >= 33) {
                    intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)
                }
                val isStorage = device != null && UsbDeviceChecker.isMassStorage(device)
                Log.d(TAG, "USB attached: ${device?.deviceName} massStorage=$isStorage")
                if (isStorage) {
                    UsbConnectionState.notifyChanged(context, attached = true)
                }
            }

            UsbManager.ACTION_USB_DEVICE_DETACHED -> {
                Log.d(TAG, "USB detached")
                UsbConnectionState.notifyChanged(context, attached = false)
            }
        }
    }

    companion object {
        private const val TAG = "UsbReceiver"
    }
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/usb/UsbConnectionState.kt`
```kotlin
package com.syncdrop.android.usb

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Process-wide observable USB connection state. The receiver pushes attach/detach
 * events here; ViewModels collect [state]. On each change we also recompute the
 * resolved mount path so the UI can auto-fill destination paths.
 */
object UsbConnectionState {

    data class State(val connected: Boolean, val mountPath: String?, val label: String?)

    private val _state = MutableStateFlow(State(connected = false, mountPath = null, label = null))
    val state: StateFlow<State> = _state.asStateFlow()

    /** Recompute from the system at startup or on demand. */
    fun refresh(context: Context) {
        val vol = UsbMountHelper.findMountedVolume(context.applicationContext)
        _state.value = State(
            connected = vol != null,
            mountPath = vol?.path,
            label = vol?.label
        )
    }

    /** Called by UsbReceiver on attach/detach. */
    fun notifyChanged(context: Context, attached: Boolean) {
        if (attached) {
            refresh(context)
        } else {
            _state.value = State(connected = false, mountPath = null, label = null)
        }
    }
}
```

### Test: `android/app/src/test/kotlin/com/syncdrop/android/usb/UsbDeviceCheckerTest.kt`
```kotlin
package com.syncdrop.android.usb

import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbInterface
import com.google.common.truth.Truth.assertThat
import io.mockk.every
import io.mockk.mockk
import org.junit.Test

class UsbDeviceCheckerTest {

    @Test
    fun `device-level mass storage class is detected`() {
        val device = mockk<UsbDevice> {
            every { deviceClass } returns UsbConstants.USB_CLASS_MASS_STORAGE
            every { interfaceCount } returns 0
        }
        assertThat(UsbDeviceChecker.isMassStorage(device)).isTrue()
    }

    @Test
    fun `interface-level mass storage class is detected`() {
        val iface = mockk<UsbInterface> {
            every { interfaceClass } returns UsbConstants.USB_CLASS_MASS_STORAGE
        }
        val device = mockk<UsbDevice> {
            every { deviceClass } returns UsbConstants.USB_CLASS_PER_INTERFACE
            every { interfaceCount } returns 1
            every { getInterface(0) } returns iface
        }
        assertThat(UsbDeviceChecker.isMassStorage(device)).isTrue()
    }

    @Test
    fun `non storage device is not detected`() {
        val iface = mockk<UsbInterface> {
            every { interfaceClass } returns UsbConstants.USB_CLASS_HID
        }
        val device = mockk<UsbDevice> {
            every { deviceClass } returns UsbConstants.USB_CLASS_HID
            every { interfaceCount } returns 1
            every { getInterface(0) } returns iface
        }
        assertThat(UsbDeviceChecker.isMassStorage(device)).isFalse()
    }
}
```

### Manual verification (hardware-dependent)
- [ ] Build & install on a phone with USB OTG support.
- [ ] Plug in a USB-C SSD via OTG. Confirm the system shows the SyncDrop "open with" prompt (from the `USB_DEVICE_ATTACHED` intent filter) and that `UsbConnectionState.state` reports `connected = true` with a non-null `mountPath` (verify via Logcat tag `UsbReceiver`).
- [ ] Unplug the SSD; confirm `connected = false`.

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/usb/ android/app/src/test/kotlin/com/syncdrop/android/usb/`
- [ ] `git commit -m "Task 7: USB mass-storage detection and mount resolution"`

---

## Task 8 — FileComparer (pure diff logic)

**Goal:** Compute which files to copy/delete. The diff core operates on in-memory maps so it is fully unit-testable; SAF/File walking is isolated behind small interfaces.

### `android/app/src/main/kotlin/com/syncdrop/android/sync/FileEntry.kt`
```kotlin
package com.syncdrop.android.sync

import android.net.Uri

data class FileEntry(
    val uri: Uri,
    val name: String,
    val size: Long,
    val lastModified: Long,
    val relativePath: String
)

data class SyncDiff(
    val toCopy: List<FileEntry>,
    val toDelete: List<String>
)

/** Lightweight, Android-free descriptor of a destination file for diffing. */
data class DestEntry(
    val relativePath: String,
    val size: Long,
    val lastModified: Long
)
```

### `android/app/src/main/kotlin/com/syncdrop/android/sync/FileComparer.kt`
```kotlin
package com.syncdrop.android.sync

import android.content.Context
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import java.io.File

/**
 * Compares a source SAF tree against a destination File tree and produces a SyncDiff.
 *
 * The pure diff algorithm ([diff]) is Android-free and unit-tested. The walking
 * methods ([walkSource], [walkDest]) adapt the platform APIs into plain maps.
 */
object FileComparer {

    /**
     * Pure diff. A source file is copied when:
     *  - the destination has no file at that relativePath, OR
     *  - dest.lastModified < source.lastModified, OR
     *  - dest.size != source.size.
     * When [mirrorMode] is true, destination files absent from the source are deleted.
     */
    fun diff(
        source: Map<String, FileEntry>,
        dest: Map<String, DestEntry>,
        mirrorMode: Boolean
    ): SyncDiff {
        val toCopy = source.values.filter { src ->
            val d = dest[src.relativePath]
            d == null || d.lastModified < src.lastModified || d.size != src.size
        }
        val toDelete = if (mirrorMode) {
            dest.keys.filter { it !in source.keys }
        } else {
            emptyList()
        }
        return SyncDiff(toCopy = toCopy, toDelete = toDelete)
    }

    /** True if [name] matches any exclusion pattern (exact name or "*.ext" glob). */
    fun isExcluded(name: String, excludePatterns: List<String>): Boolean {
        for (pattern in excludePatterns) {
            if (pattern.isEmpty()) continue
            if (pattern.startsWith("*.")) {
                val ext = pattern.substring(1) // ".ext"
                if (name.endsWith(ext, ignoreCase = true)) return true
            } else if (name.equals(pattern, ignoreCase = false)) {
                return true
            }
        }
        return false
    }

    /**
     * Walks a SAF source tree rooted at [rootUri], returning relativePath -> FileEntry.
     * [rootPrefix] is prepended to relative paths so multiple sources can share a dest
     * without collisions (uses the root document's display name).
     */
    fun walkSource(
        context: Context,
        rootUri: Uri,
        excludePatterns: List<String>
    ): Map<String, FileEntry> {
        val result = LinkedHashMap<String, FileEntry>()
        val root = DocumentFile.fromTreeUri(context, rootUri) ?: return result
        val rootName = root.name ?: "source"
        walkSourceRecursive(root, rootName, excludePatterns, result)
        return result
    }

    private fun walkSourceRecursive(
        dir: DocumentFile,
        relativeDir: String,
        excludePatterns: List<String>,
        out: MutableMap<String, FileEntry>
    ) {
        for (child in dir.listFiles()) {
            val name = child.name ?: continue
            if (isExcluded(name, excludePatterns)) continue
            val childRelative = "$relativeDir/$name"
            if (child.isDirectory) {
                walkSourceRecursive(child, childRelative, excludePatterns, out)
            } else {
                out[childRelative] = FileEntry(
                    uri = child.uri,
                    name = name,
                    size = child.length(),
                    lastModified = child.lastModified(),
                    relativePath = childRelative
                )
            }
        }
    }

    /** Walks the destination File tree under [destRoot], returning relativePath -> DestEntry. */
    fun walkDest(destRoot: String): Map<String, DestEntry> {
        val result = LinkedHashMap<String, DestEntry>()
        val root = File(destRoot)
        if (!root.exists() || !root.isDirectory) return result
        walkDestRecursive(root, root, result)
        return result
    }

    private fun walkDestRecursive(root: File, dir: File, out: MutableMap<String, DestEntry>) {
        val children = dir.listFiles() ?: return
        for (child in children) {
            if (child.isDirectory) {
                walkDestRecursive(root, child, out)
            } else {
                val relative = root.toURI().relativize(child.toURI()).path
                out[relative] = DestEntry(
                    relativePath = relative,
                    size = child.length(),
                    lastModified = child.lastModified()
                )
            }
        }
    }
}
```

### Test: `android/app/src/test/kotlin/com/syncdrop/android/sync/FileComparerTest.kt`
```kotlin
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
```

### Verification
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:testDebugUnitTest --tests "com.syncdrop.android.sync.FileComparerTest"`

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/sync/FileEntry.kt android/app/src/main/kotlin/com/syncdrop/android/sync/FileComparer.kt android/app/src/test/kotlin/com/syncdrop/android/sync/FileComparerTest.kt`
- [ ] `git commit -m "Task 8: FileComparer diff algorithm and tree walking"`

---

## Task 9 — FileCopier

**Goal:** Copy a single `FileEntry` to the destination, creating directories and preserving timestamps.

### `android/app/src/main/kotlin/com/syncdrop/android/sync/FileCopier.kt`
```kotlin
package com.syncdrop.android.sync

import android.content.Context
import android.util.Log
import java.io.File
import java.io.FileOutputStream

/**
 * Copies a source SAF file to a destination File tree.
 *
 * NOTE (known limitation): on target SDK 34, direct File writes to a removable
 * USB volume can be blocked by scoped storage. The copy fails gracefully (returns
 * false) and the caller records the failure. A SAF-based destination is future work.
 */
object FileCopier {

    private const val BUFFER_SIZE = 64 * 1024

    fun copy(entry: FileEntry, destRoot: String, context: Context): Boolean {
        val destFile = File(destRoot, entry.relativePath)
        return try {
            destFile.parentFile?.let { parent ->
                if (!parent.exists() && !parent.mkdirs()) {
                    Log.e(TAG, "Failed to create dirs: ${parent.absolutePath}")
                    return false
                }
            }
            context.contentResolver.openInputStream(entry.uri)?.use { input ->
                FileOutputStream(destFile).use { output ->
                    val buffer = ByteArray(BUFFER_SIZE)
                    var read: Int
                    while (input.read(buffer).also { read = it } >= 0) {
                        output.write(buffer, 0, read)
                    }
                    output.flush()
                }
            } ?: run {
                Log.e(TAG, "Could not open input stream for ${entry.uri}")
                return false
            }
            // Preserve source modification time so future diffs are stable.
            if (entry.lastModified > 0) {
                destFile.setLastModified(entry.lastModified)
            }
            true
        } catch (e: Exception) {
            Log.e(TAG, "Copy failed for ${entry.relativePath}: ${e.message}", e)
            false
        }
    }

    /** Deletes a destination file by relative path (used in mirror mode). */
    fun delete(relativePath: String, destRoot: String): Boolean {
        return try {
            val f = File(destRoot, relativePath)
            if (f.exists()) f.delete() else true
        } catch (e: Exception) {
            Log.e(TAG, "Delete failed for $relativePath: ${e.message}", e)
            false
        }
    }

    private const val TAG = "FileCopier"
}
```

### Test: `android/app/src/test/kotlin/com/syncdrop/android/sync/FileCopierDeleteTest.kt`
```kotlin
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
```

> The `copy` path depends on `ContentResolver`/`Uri` (Android framework) so it is verified manually on-device (Task 13 manual steps). The `delete` path is pure file IO and is unit-tested above.

### Verification
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:testDebugUnitTest --tests "com.syncdrop.android.sync.FileCopierDeleteTest"`

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/sync/FileCopier.kt android/app/src/test/kotlin/com/syncdrop/android/sync/FileCopierDeleteTest.kt`
- [ ] `git commit -m "Task 9: FileCopier copy and delete"`

---

## Task 10 — SyncWorker (HiltWorker)

**Goal:** Background sync worker that walks → diffs → copies, posting progress and recording a `SyncRecord`.

### `android/app/src/main/kotlin/com/syncdrop/android/sync/SyncWorker.kt`
```kotlin
package com.syncdrop.android.sync

import android.app.Notification
import android.content.Context
import android.content.pm.ServiceInfo
import android.net.Uri
import androidx.core.app.NotificationCompat
import androidx.hilt.work.HiltWorker
import androidx.work.CoroutineWorker
import androidx.work.ForegroundInfo
import androidx.work.WorkerParameters
import androidx.work.workDataOf
import com.syncdrop.android.R
import com.syncdrop.android.SyncDropApplication
import com.syncdrop.android.data.model.SyncRecord
import com.syncdrop.android.data.repository.HistoryRepository
import com.syncdrop.android.data.repository.JsonStringList
import com.syncdrop.android.data.repository.ProfileRepository
import dagger.assisted.Assisted
import dagger.assisted.AssistedInject
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.ensureActive
import kotlin.coroutines.coroutineContext

@HiltWorker
class SyncWorker @AssistedInject constructor(
    @Assisted appContext: Context,
    @Assisted params: WorkerParameters,
    private val profileRepository: ProfileRepository,
    private val historyRepository: HistoryRepository
) : CoroutineWorker(appContext, params) {

    override suspend fun doWork(): Result = coroutineScope {
        val profileId = inputData.getString(KEY_PROFILE_ID)
            ?: return@coroutineScope Result.failure()
        val destOverride = inputData.getString(KEY_USB_DEST_PATH)

        val profile = profileRepository.getProfile(profileId)
            ?: return@coroutineScope Result.failure()

        val destPath = destOverride ?: profile.destPath
        val startedAt = System.currentTimeMillis()

        setForeground(buildForegroundInfo(0, 0, ""))

        val excludes = JsonStringList.decode(profile.excludePatterns)
        val sourceUris = JsonStringList.decode(profile.sourceUris)

        // 1) Walk all sources into one combined map.
        val source = LinkedHashMap<String, FileEntry>()
        for (uriString in sourceUris) {
            val uri = Uri.parse(uriString)
            source.putAll(FileComparer.walkSource(applicationContext, uri, excludes))
        }

        // 2) Walk destination and diff.
        val dest = FileComparer.walkDest(destPath)
        val diff = FileComparer.diff(source, dest, profile.mirrorMode)

        val total = diff.toCopy.size
        var copied = 0
        var bytes = 0L
        var failure: String? = null

        try {
            // 3) Copy.
            for (entry in diff.toCopy) {
                coroutineContext.ensureActive() // honor cancellation
                setProgress(
                    workDataOf(
                        KEY_FILES_DONE to copied,
                        KEY_FILES_TOTAL to total,
                        KEY_CURRENT_FILE to entry.name
                    )
                )
                setForeground(buildForegroundInfo(copied, total, entry.name))
                val ok = FileCopier.copy(entry, destPath, applicationContext)
                if (ok) {
                    copied++
                    bytes += entry.size
                } else if (failure == null) {
                    failure = "Failed to copy ${entry.relativePath}"
                }
            }

            // 4) Mirror deletions.
            if (profile.mirrorMode) {
                for (rel in diff.toDelete) {
                    coroutineContext.ensureActive()
                    FileCopier.delete(rel, destPath)
                }
            }
        } catch (e: Exception) {
            failure = e.message ?: "Sync interrupted"
        }

        val succeeded = failure == null
        historyRepository.add(
            SyncRecord(
                profileId = profileId,
                startedAt = startedAt,
                finishedAt = System.currentTimeMillis(),
                filesCopied = copied,
                bytesTransferred = bytes,
                succeeded = succeeded,
                errorMessage = failure
            )
        )

        if (succeeded) {
            Result.success(
                workDataOf(KEY_FILES_DONE to copied, KEY_FILES_TOTAL to total)
            )
        } else {
            Result.failure(workDataOf(KEY_ERROR to (failure ?: "Unknown error")))
        }
    }

    private fun buildForegroundInfo(done: Int, total: Int, current: String): ForegroundInfo {
        val text = if (total > 0) "$done / $total — $current" else "Preparing…"
        val notification: Notification = NotificationCompat.Builder(
            applicationContext,
            SyncDropApplication.SYNC_CHANNEL_ID
        )
            .setContentTitle(applicationContext.getString(R.string.app_name))
            .setContentText(text)
            .setSmallIcon(android.R.drawable.stat_sys_upload)
            .setOngoing(true)
            .setProgress(total.coerceAtLeast(1), done, total == 0)
            .build()

        return if (android.os.Build.VERSION.SDK_INT >= 34) {
            ForegroundInfo(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
        } else {
            ForegroundInfo(NOTIFICATION_ID, notification)
        }
    }

    companion object {
        const val KEY_PROFILE_ID = "PROFILE_ID"
        const val KEY_USB_DEST_PATH = "USB_DEST_PATH"
        const val KEY_FILES_DONE = "filesDone"
        const val KEY_FILES_TOTAL = "filesTotal"
        const val KEY_CURRENT_FILE = "currentFile"
        const val KEY_ERROR = "error"
        const val NOTIFICATION_ID = 4201
    }
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/sync/SyncScheduler.kt`
```kotlin
package com.syncdrop.android.sync

import android.content.Context
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.workDataOf
import java.util.UUID

/** Helper to enqueue a one-time sync and return its WorkRequest id. */
object SyncScheduler {

    fun enqueueSync(context: Context, profileId: String, destPath: String): UUID {
        val request = OneTimeWorkRequestBuilder<SyncWorker>()
            .setInputData(
                workDataOf(
                    SyncWorker.KEY_PROFILE_ID to profileId,
                    SyncWorker.KEY_USB_DEST_PATH to destPath
                )
            )
            .addTag("sync:$profileId")
            .build()
        WorkManager.getInstance(context).enqueue(request)
        return request.id
    }

    fun cancel(context: Context, workId: UUID) {
        WorkManager.getInstance(context).cancelWorkById(workId)
    }
}
```

### Verification
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:assembleDebug` — confirms `@HiltWorker` + assisted injection compiles against the `HiltWorkerFactory` wired in Task 5.
- [ ] Manual (on-device, Task 13): trigger a sync; confirm the foreground notification appears with progress and a `SyncRecord` row is written.

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/sync/SyncWorker.kt android/app/src/main/kotlin/com/syncdrop/android/sync/SyncScheduler.kt`
- [ ] `git commit -m "Task 10: SyncWorker (HiltWorker) and SyncScheduler"`

---

## Task 11 — ViewModels (home, profile, progress, history)

**Goal:** All four ViewModels, each `@HiltViewModel`, exposing `StateFlow` UI state.

### `android/app/src/main/kotlin/com/syncdrop/android/ui/home/HomeViewModel.kt`
```kotlin
package com.syncdrop.android.ui.home

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.model.SyncRecord
import com.syncdrop.android.data.repository.HistoryRepository
import com.syncdrop.android.data.repository.ProfileRepository
import com.syncdrop.android.sync.SyncScheduler
import com.syncdrop.android.usb.UsbConnectionState
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import java.util.UUID
import javax.inject.Inject

data class HomeUiState(
    val profiles: List<SyncProfile> = emptyList(),
    val latestByProfile: Map<String, SyncRecord> = emptyMap(),
    val usbConnected: Boolean = false,
    val usbMountPath: String? = null,
    val usbLabel: String? = null
)

@HiltViewModel
class HomeViewModel @Inject constructor(
    application: Application,
    private val profileRepository: ProfileRepository,
    private val historyRepository: HistoryRepository
) : AndroidViewModel(application) {

    init {
        // Recompute USB state when the screen's VM is created.
        UsbConnectionState.refresh(application)
    }

    val uiState: StateFlow<HomeUiState> =
        combine(
            profileRepository.observeProfiles(),
            historyRepository.observeRecords(),
            UsbConnectionState.state
        ) { profiles, records, usb ->
            val latest = records
                .groupBy { it.profileId }
                .mapValues { (_, recs) -> recs.maxByOrNull { it.startedAt }!! }
            HomeUiState(
                profiles = profiles,
                latestByProfile = latest,
                usbConnected = usb.connected,
                usbMountPath = usb.mountPath,
                usbLabel = usb.label
            )
        }.stateIn(
            scope = viewModelScope,
            started = SharingStarted.WhileSubscribed(5_000),
            initialValue = HomeUiState()
        )

    fun deleteProfile(profile: SyncProfile) {
        viewModelScope.launch { profileRepository.delete(profile) }
    }

    /** Enqueues a sync for [profile] and returns the WorkRequest id to navigate to. */
    fun syncNow(profile: SyncProfile): UUID {
        // If the SSD remounted at a different path than the one stored on the
        // profile, rebuild the destination from the live mount + the profile's
        // folder name. Otherwise use the stored destPath as-is.
        val mount = uiState.value.usbMountPath
        val destPath = if (mount != null && !profile.destPath.startsWith(mount)) {
            "$mount/" + profile.destPath.substringAfterLast('/').ifBlank { "SyncDrop" }
        } else {
            profile.destPath
        }
        return SyncScheduler.enqueueSync(getApplication(), profile.id, destPath)
    }

    fun refreshUsb() {
        UsbConnectionState.refresh(getApplication())
    }
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/ui/profile/ProfileEditViewModel.kt`
```kotlin
package com.syncdrop.android.ui.profile

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.repository.JsonStringList
import com.syncdrop.android.data.repository.ProfileRepository
import com.syncdrop.android.usb.UsbConnectionState
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject

data class ProfileEditUiState(
    val id: String? = null,
    val name: String = "",
    val sourceUris: List<String> = emptyList(),
    val destPath: String = "",
    val mirrorMode: Boolean = false,
    val autoSync: Boolean = false,
    val excludePatterns: List<String> = listOf(".nomedia", ".thumbnails"),
    val saved: Boolean = false
)

@HiltViewModel
class ProfileEditViewModel @Inject constructor(
    application: Application,
    private val repository: ProfileRepository
) : AndroidViewModel(application) {

    private val _state = MutableStateFlow(ProfileEditUiState())
    val state: StateFlow<ProfileEditUiState> = _state.asStateFlow()

    fun load(profileId: String?) {
        if (profileId == null) {
            // New profile: prefill dest from the connected SSD if available.
            val mount = UsbConnectionState.state.value.mountPath
            _state.update {
                it.copy(destPath = mount?.let { m -> "$m/SyncDrop" } ?: "")
            }
            return
        }
        viewModelScope.launch {
            val p = repository.getProfile(profileId) ?: return@launch
            _state.value = ProfileEditUiState(
                id = p.id,
                name = p.name,
                sourceUris = JsonStringList.decode(p.sourceUris),
                destPath = p.destPath,
                mirrorMode = p.mirrorMode,
                autoSync = p.autoSync,
                excludePatterns = JsonStringList.decode(p.excludePatterns)
            )
        }
    }

    fun onNameChange(value: String) = _state.update { it.copy(name = value) }

    fun onDestPathChange(value: String) = _state.update { it.copy(destPath = value) }

    fun onMirrorToggle(value: Boolean) = _state.update { it.copy(mirrorMode = value) }

    fun onAutoSyncToggle(value: Boolean) = _state.update { it.copy(autoSync = value) }

    fun addSource(uriString: String) = _state.update {
        if (it.sourceUris.contains(uriString)) it
        else it.copy(sourceUris = it.sourceUris + uriString)
    }

    fun removeSource(uriString: String) = _state.update {
        it.copy(sourceUris = it.sourceUris - uriString)
    }

    fun addExcludePattern(pattern: String) = _state.update {
        val trimmed = pattern.trim()
        if (trimmed.isEmpty() || it.excludePatterns.contains(trimmed)) it
        else it.copy(excludePatterns = it.excludePatterns + trimmed)
    }

    fun removeExcludePattern(pattern: String) = _state.update {
        it.copy(excludePatterns = it.excludePatterns - pattern)
    }

    fun save() {
        val s = _state.value
        if (s.name.isBlank()) return
        viewModelScope.launch {
            val profile = if (s.id != null) {
                SyncProfile(
                    id = s.id,
                    name = s.name,
                    sourceUris = JsonStringList.encode(s.sourceUris),
                    destPath = s.destPath,
                    mirrorMode = s.mirrorMode,
                    autoSync = s.autoSync,
                    excludePatterns = JsonStringList.encode(s.excludePatterns)
                )
            } else {
                SyncProfile(
                    name = s.name,
                    sourceUris = JsonStringList.encode(s.sourceUris),
                    destPath = s.destPath,
                    mirrorMode = s.mirrorMode,
                    autoSync = s.autoSync,
                    excludePatterns = JsonStringList.encode(s.excludePatterns)
                )
            }
            repository.upsert(profile)
            _state.update { it.copy(saved = true) }
        }
    }

    fun delete() {
        val id = _state.value.id ?: return
        viewModelScope.launch {
            repository.deleteById(id)
            _state.update { it.copy(saved = true) }
        }
    }
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/ui/progress/SyncProgressViewModel.kt`
```kotlin
package com.syncdrop.android.ui.progress

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import androidx.work.WorkInfo
import androidx.work.WorkManager
import com.syncdrop.android.sync.SyncWorker
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.util.UUID
import javax.inject.Inject

data class SyncProgressUiState(
    val filesDone: Int = 0,
    val filesTotal: Int = 0,
    val currentFile: String = "",
    val finished: Boolean = false,
    val succeeded: Boolean = false,
    val error: String? = null
)

@HiltViewModel
class SyncProgressViewModel @Inject constructor(
    application: Application
) : AndroidViewModel(application) {

    private val _state = MutableStateFlow(SyncProgressUiState())
    val state: StateFlow<SyncProgressUiState> = _state.asStateFlow()

    private var workId: UUID? = null

    fun observe(id: UUID) {
        workId = id
        viewModelScope.launch {
            WorkManager.getInstance(getApplication())
                .getWorkInfoByIdFlow(id)
                .collect { info -> handle(info) }
        }
    }

    private fun handle(info: WorkInfo?) {
        if (info == null) return
        when (info.state) {
            WorkInfo.State.RUNNING -> {
                val p = info.progress
                _state.value = _state.value.copy(
                    filesDone = p.getInt(SyncWorker.KEY_FILES_DONE, _state.value.filesDone),
                    filesTotal = p.getInt(SyncWorker.KEY_FILES_TOTAL, _state.value.filesTotal),
                    currentFile = p.getString(SyncWorker.KEY_CURRENT_FILE) ?: _state.value.currentFile
                )
            }
            WorkInfo.State.SUCCEEDED -> {
                _state.value = _state.value.copy(
                    filesDone = info.outputData.getInt(SyncWorker.KEY_FILES_DONE, _state.value.filesDone),
                    filesTotal = info.outputData.getInt(SyncWorker.KEY_FILES_TOTAL, _state.value.filesTotal),
                    finished = true,
                    succeeded = true
                )
            }
            WorkInfo.State.FAILED -> {
                _state.value = _state.value.copy(
                    finished = true,
                    succeeded = false,
                    error = info.outputData.getString(SyncWorker.KEY_ERROR)
                )
            }
            WorkInfo.State.CANCELLED -> {
                _state.value = _state.value.copy(
                    finished = true,
                    succeeded = false,
                    error = "Cancelled"
                )
            }
            else -> { /* ENQUEUED, BLOCKED — keep current state */ }
        }
    }

    fun cancel() {
        workId?.let { WorkManager.getInstance(getApplication()).cancelWorkById(it) }
    }
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/ui/history/HistoryViewModel.kt`
```kotlin
package com.syncdrop.android.ui.history

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.model.SyncRecord
import com.syncdrop.android.data.repository.HistoryRepository
import com.syncdrop.android.data.repository.ProfileRepository
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import javax.inject.Inject

data class HistoryRow(
    val record: SyncRecord,
    val profileName: String
)

data class HistoryUiState(
    val rows: List<HistoryRow> = emptyList()
)

@HiltViewModel
class HistoryViewModel @Inject constructor(
    private val historyRepository: HistoryRepository,
    private val profileRepository: ProfileRepository
) : ViewModel() {

    val uiState: StateFlow<HistoryUiState> =
        combine(
            historyRepository.observeRecords(),
            profileRepository.observeProfiles()
        ) { records, profiles ->
            val nameById: Map<String, String> =
                profiles.associate { p: SyncProfile -> p.id to p.name }
            HistoryUiState(
                rows = records.map { rec ->
                    HistoryRow(record = rec, profileName = nameById[rec.profileId] ?: "(deleted)")
                }
            )
        }.stateIn(
            scope = viewModelScope,
            started = SharingStarted.WhileSubscribed(5_000),
            initialValue = HistoryUiState()
        )

    fun clearHistory() {
        viewModelScope.launch { historyRepository.clear() }
    }
}
```

### Test: `android/app/src/test/kotlin/com/syncdrop/android/ui/profile/ProfileEditMappingTest.kt`
```kotlin
package com.syncdrop.android.ui.profile

import com.syncdrop.android.data.model.SyncProfile
import com.syncdrop.android.data.repository.JsonStringList
import com.google.common.truth.Truth.assertThat
import org.junit.Test

/**
 * Verifies the pure mapping between ProfileEditUiState fields and the persisted
 * SyncProfile JSON columns. (The ViewModel save() uses these same transforms.)
 */
class ProfileEditMappingTest {

    @Test
    fun `ui source list maps to json column and back`() {
        val uris = listOf("content://a", "content://b")
        val encoded = JsonStringList.encode(uris)
        val profile = SyncProfile(name = "p", sourceUris = encoded, destPath = "/x")
        assertThat(JsonStringList.decode(profile.sourceUris)).containsExactlyElementsIn(uris).inOrder()
    }

    @Test
    fun `exclude patterns map to json column and back`() {
        val patterns = listOf(".nomedia", "*.tmp")
        val encoded = JsonStringList.encode(patterns)
        val profile = SyncProfile(name = "p", sourceUris = "[]", destPath = "/x", excludePatterns = encoded)
        assertThat(JsonStringList.decode(profile.excludePatterns)).containsExactlyElementsIn(patterns).inOrder()
    }
}
```

### Verification
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:testDebugUnitTest --tests "com.syncdrop.android.ui.profile.*"`
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:compileDebugKotlin` — confirms all four `@HiltViewModel`s compile.

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/ui/home/HomeViewModel.kt android/app/src/main/kotlin/com/syncdrop/android/ui/profile/ProfileEditViewModel.kt android/app/src/main/kotlin/com/syncdrop/android/ui/progress/SyncProgressViewModel.kt android/app/src/main/kotlin/com/syncdrop/android/ui/history/HistoryViewModel.kt android/app/src/test/kotlin/com/syncdrop/android/ui/profile/ProfileEditMappingTest.kt`
- [ ] `git commit -m "Task 11: Home, ProfileEdit, SyncProgress, History ViewModels"`

---

## Task 12 — Compose screens (home, profile, progress, history)

**Goal:** All four screens plus a shared format util. Screens take their ViewModel via `hiltViewModel()` and navigation callbacks as lambdas.

### `android/app/src/main/kotlin/com/syncdrop/android/ui/Formatters.kt`
```kotlin
package com.syncdrop.android.ui

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

object Formatters {

    private val dateFmt = SimpleDateFormat("MMM d, yyyy HH:mm", Locale.getDefault())

    fun formatDate(epochMillis: Long): String =
        if (epochMillis <= 0L) "—" else dateFmt.format(Date(epochMillis))

    fun formatBytes(bytes: Long): String {
        if (bytes < 1024) return "$bytes B"
        val units = listOf("KB", "MB", "GB", "TB")
        var value = bytes.toDouble() / 1024.0
        var idx = 0
        while (value >= 1024.0 && idx < units.size - 1) {
            value /= 1024.0
            idx++
        }
        return String.format(Locale.getDefault(), "%.1f %s", value, units[idx])
    }

    fun formatDuration(startMillis: Long, endMillis: Long): String {
        val secs = ((endMillis - startMillis) / 1000).coerceAtLeast(0)
        val m = secs / 60
        val s = secs % 60
        return if (m > 0) "${m}m ${s}s" else "${s}s"
    }
}
```

### Test: `android/app/src/test/kotlin/com/syncdrop/android/ui/FormattersTest.kt`
```kotlin
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
```

### `android/app/src/main/kotlin/com/syncdrop/android/ui/home/HomeScreen.kt`
```kotlin
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
import com.syncdrop.android.ui.theme.ErrorRed
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
                    ProfileCard(
                        profile = profile,
                        lastSyncMillis = state.latestByProfile[profile.id]?.startedAt ?: 0L,
                        syncEnabled = state.usbConnected,
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
    Card(
        modifier = Modifier
            .fillMaxWidth()
            .combinedClickable(onClick = onClick, onLongClick = onLongPress)
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(text = profile.name, style = MaterialTheme.typography.titleLarge)
            Text(
                text = "$sourceCount folder(s) → ${profile.destPath}",
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
```

### `android/app/src/main/kotlin/com/syncdrop/android/ui/profile/ProfileEditScreen.kt`
```kotlin
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

            item {
                OutlinedTextField(
                    value = state.destPath,
                    onValueChange = viewModel::onDestPathChange,
                    label = { Text("Destination path on SSD") },
                    modifier = Modifier.fillMaxWidth()
                )
            }

            item {
                Text(
                    text = stringResource(R.string.storage_note),
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
```

### `android/app/src/main/kotlin/com/syncdrop/android/ui/progress/SyncProgressScreen.kt`
```kotlin
package com.syncdrop.android.ui.progress

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.util.UUID

@Composable
fun SyncProgressScreen(
    workId: UUID,
    onDone: () -> Unit,
    viewModel: SyncProgressViewModel = hiltViewModel()
) {
    val state by viewModel.state.collectAsStateWithLifecycle()

    LaunchedEffect(workId) { viewModel.observe(workId) }
    LaunchedEffect(state.finished) { if (state.finished) onDone() }

    Scaffold { padding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(padding),
            contentAlignment = Alignment.Center
        ) {
            Column(
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(16.dp)
            ) {
                val progress = if (state.filesTotal > 0) {
                    state.filesDone.toFloat() / state.filesTotal.toFloat()
                } else {
                    null
                }
                if (progress != null) {
                    CircularProgressIndicator(
                        progress = { progress },
                        modifier = Modifier.size(96.dp)
                    )
                } else {
                    CircularProgressIndicator(modifier = Modifier.size(96.dp))
                }

                Text(
                    text = "${state.filesDone} / ${state.filesTotal} files",
                    style = MaterialTheme.typography.titleLarge
                )
                Text(
                    text = state.currentFile,
                    style = MaterialTheme.typography.bodyLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 32.dp)
                )

                Button(onClick = { viewModel.cancel() }) { Text("Cancel") }
            }
        }
    }
}
```

### `android/app/src/main/kotlin/com/syncdrop/android/ui/history/HistoryScreen.kt`
```kotlin
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
```

### Verification
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:testDebugUnitTest --tests "com.syncdrop.android.ui.FormattersTest"`
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:compileDebugKotlin` — confirms all composables compile (will fully build once MainActivity wires them in Task 13).
- [ ] Manual: previews / on-device once Task 13 is done.

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/ui/Formatters.kt android/app/src/main/kotlin/com/syncdrop/android/ui/home/HomeScreen.kt android/app/src/main/kotlin/com/syncdrop/android/ui/profile/ProfileEditScreen.kt android/app/src/main/kotlin/com/syncdrop/android/ui/progress/SyncProgressScreen.kt android/app/src/main/kotlin/com/syncdrop/android/ui/history/HistoryScreen.kt android/app/src/test/kotlin/com/syncdrop/android/ui/FormattersTest.kt`
- [ ] `git commit -m "Task 12: Compose screens (home, profile, progress, history)"`

---

## Task 13 — MainActivity + NavHost (single activity) and end-to-end manual verification

**Goal:** Wire all screens into a single-activity NavHost, request the notifications permission, and verify the full flow on-device.

### `android/app/src/main/kotlin/com/syncdrop/android/MainActivity.kt`
```kotlin
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
```

### Verification (full build + on-device end-to-end)
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:testDebugUnitTest` — entire unit suite passes (models, JSON, repos, USB checker, comparer, copier-delete, formatters, profile mapping).
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:assembleDebug` — APK builds.
- [ ] `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:installDebug` — installs on a connected device/emulator.
- [ ] **Manual flow on a real phone with USB OTG + a USB-C SSD:**
  - [ ] Launch app → Home shows "No SSD connected" banner, empty profile list, FAB visible.
  - [ ] Tap FAB → New Profile. Enter a name. Tap "Add Folder" → SAF picker → choose a phone folder (e.g. Camera). Confirm it appears in the source list and read permission is persisted (no crash on later sync).
  - [ ] Plug in the SSD → system shows SyncDrop "open with" prompt; Home banner turns green "SSD connected". Destination path auto-fills with `<mount>/SyncDrop` on a freshly opened New Profile.
  - [ ] Save profile → returns to Home, card visible with folder count and dest path.
  - [ ] Tap "Sync Now" (enabled because SSD connected) → navigates to Progress; circular indicator + "X / Y files" + current filename update live; foreground notification shows progress.
  - [ ] Let it finish → auto-navigates back to Home.
  - [ ] Open History → row shows profile name, date, file count, size, duration, green check.
  - [ ] Re-run "Sync Now" with no changes → 0 files copied (diff skip works).
  - [ ] Modify a source file, sync again → only the changed file copies.
  - [ ] Enable Mirror mode, delete a source file, sync → corresponding dest file removed.
  - [ ] During a large sync, tap "Cancel" → worker stops; History row shows failure/"Cancelled".
  - [ ] **Storage limitation check:** if writes to the SSD path fail (scoped-storage block on this device), confirm the History row records the failure with an error message and the UI surfaces the `storage_note` guidance. (This is the documented known limitation; not a bug in the plan.)
  - [ ] Long-press a profile card → profile deleted.
  - [ ] History → "Clear" → list empties.

### Commit
- [ ] `git add android/app/src/main/kotlin/com/syncdrop/android/MainActivity.kt`
- [ ] `git commit -m "Task 13: MainActivity NavHost wiring and end-to-end verification"`

---

## Appendix A — Full unit test inventory

| Test file | Covers (pure logic) |
|-----------|---------------------|
| `data/model/SyncProfileTest.kt` | UUID defaults, distinct ids, default excludes |
| `data/repository/JsonStringListTest.kt` | URI/pattern JSON round-trip, malformed input |
| `data/repository/ProfileRepositoryTest.kt` | DAO delegation (MockK) |
| `usb/UsbDeviceCheckerTest.kt` | mass-storage detection at device & interface level (MockK) |
| `sync/FileComparerTest.kt` | diff: new/unchanged/newer/size-mismatch, mirror delete, exclude globs |
| `sync/FileCopierDeleteTest.kt` | file delete (TemporaryFolder) |
| `ui/profile/ProfileEditMappingTest.kt` | UI list ↔ JSON column mapping |
| `ui/FormattersTest.kt` | bytes/duration/date formatting |

Run all: `cd /Users/padidamabhinay/SyncDrop/android && ./gradlew :app:testDebugUnitTest`

## Appendix B — Manual-only surfaces (framework/hardware-bound)

- USB attach/detach broadcast + `UsbConnectionState` updates (real device).
- `UsbMountHelper` path resolution (depends on device storage layout / `StorageManager`).
- `FileCopier.copy` (needs `ContentResolver`/SAF `Uri`).
- `SyncWorker` end-to-end (WorkManager + foreground notification).
- All Compose screens (visual + navigation).

## Appendix C — Known limitations & future work

1. **Removable-volume writes under scoped storage (target SDK 34).** The plan writes the destination via `java.io.File`/`FileOutputStream` to the SSD mount path, per spec. On many devices this is blocked for non-privileged apps. Future work: let the user grant a SAF tree on the SSD (`ACTION_OPEN_DOCUMENT_TREE` against the volume root) and rewrite `FileCopier` + `FileComparer.walkDest` to use `DocumentFile` for the destination. The diff core (`FileComparer.diff`) is already destination-agnostic and would not change.
2. **Auto-sync trigger.** `autoSync` is persisted and read (`getAutoSyncProfiles`), but the plan does not yet auto-enqueue on USB attach. Future work: in `UsbReceiver` (or a `WorkManager` `OneTimeWork` chained off the attach event), enqueue syncs for all `autoSync` profiles when an SSD mounts.
3. **Multiple-source path collisions.** Sources are namespaced under each tree's root display name; two distinct trees with the same root name could still collide. Future work: prefix with a stable per-source id.
4. **USB permission grant.** Detection uses the attach broadcast + `StorageManager`; for raw `UsbManager` device access (not needed for mass-storage-mounted volumes) a `requestPermission` flow would be added.
5. **Receiver vs. activity delivery of `USB_DEVICE_ATTACHED`.** Android delivers `ACTION_USB_DEVICE_ATTACHED` only to **activities** declaring the `<intent-filter>` + `usb_device_filter` meta-data — **not** to manifest-registered `<receiver>`s. So in practice the connected-state path that actually executes is: `MainActivity` launches via the activity filter, and `HomeViewModel.init` calls `UsbConnectionState.refresh(...)` (which also re-runs whenever the Home VM is recreated). `UsbReceiver.onReceive`'s ATTACHED branch may never fire; its DETACHED branch is reliable. `UsbReceiver` is retained per spec and is the natural home for future auto-sync-on-attach logic if registered dynamically (e.g. via a foreground service or `registerReceiver` while the app is in the foreground).
