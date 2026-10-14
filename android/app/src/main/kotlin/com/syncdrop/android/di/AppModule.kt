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
