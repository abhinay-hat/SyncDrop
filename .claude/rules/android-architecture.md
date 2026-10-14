---
paths:
  - "android/**"
---

### Android Architecture

Single-activity Jetpack Compose application (`android/`):

- **Data Layer** (`data/`): Room entities `SyncProfile` and `SyncRecord`, DAOs `SyncProfileDao` and `SyncRecordDao`, `AppDatabase`, `ProfileRepository`, `HistoryRepository`, and `JsonStringList` serializer.
- **DI** (`di/`): Hilt `AppModule` providing Room database and DAOs.
- **Application** (`SyncDropApplication`): `@HiltAndroidApp` providing `HiltWorkerFactory` for WorkManager and initializing low-importance progress notification channels.
- **USB Detection** (`usb/`): `UsbDeviceChecker` (mass storage class 0x08 filter), `UsbMountHelper` (`StorageManager` removable volume path resolution), `UsbReceiver` (broadcast receiver for attach/detach), and observable `UsbConnectionState`.
- **Sync Engine** (`sync/`): `FileComparer` (pure diff logic for additions, updates, mirror deletions, and glob excludes; SAF DocumentFile and destination File tree walking), `FileCopier` (stream-based copying with timestamp preservation), `SyncWorker` (`@HiltWorker` foreground execution with live progress reporting), and `SyncScheduler`.
- **UI & Navigation** (`ui/`): Single `MainActivity` with Compose `NavHost` (`home`, `profile/new`, `profile/{id}`, `progress/{workId}`, `history`), ViewModels (`HomeViewModel`, `ProfileEditViewModel`, `SyncProgressViewModel`, `HistoryViewModel`), Material 3 theme, and SAF tree picker.

Tests live in `android/app/src/test/kotlin/com/syncdrop/android/`.
