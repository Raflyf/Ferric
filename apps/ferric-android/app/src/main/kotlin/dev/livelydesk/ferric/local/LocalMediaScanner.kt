package dev.livelydesk.ferric.local

import android.content.ContentUris
import android.content.Context
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Build
import android.provider.MediaStore
import androidx.documentfile.provider.DocumentFile
import java.util.Locale
import uniffi.ferric_mobile_ffi.FfiLocalScanEntry

/**
 * Local Lossless & audio library scanner backed exclusively by Android Scoped Storage
 * (`MediaStore.Audio` + SAF `ACTION_OPEN_DOCUMENT_TREE` `DocumentFile`), producing `content://`
 * URIs decoded natively in Rust via [dev.livelydesk.ferric.core.AndroidContentOpener].
 */
object LocalMediaScanner {
    private val SUPPORTED_EXTENSIONS = setOf(
        "flac", "mp3", "m4a", "aac", "alac", "opus", "ogg", "oga", "wav", "aiff", "aif", "webm",
    )

    fun scanMediaStore(context: Context): List<FfiLocalScanEntry> {
        val resolver = context.contentResolver
        val collection = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            MediaStore.Audio.Media.getContentUri(MediaStore.VOLUME_EXTERNAL)
        } else {
            MediaStore.Audio.Media.EXTERNAL_CONTENT_URI
        }
        val projection = buildList {
            add(MediaStore.Audio.Media._ID)
            add(MediaStore.Audio.Media.TITLE)
            add(MediaStore.Audio.Media.ARTIST)
            add(MediaStore.Audio.Media.ALBUM)
            add(MediaStore.Audio.Media.DURATION)
            add(MediaStore.Audio.Media.MIME_TYPE)
            add(MediaStore.Audio.Media.DISPLAY_NAME)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                add(MediaStore.Audio.Media.BITRATE)
            }
        }.toTypedArray()

        val selection = "${MediaStore.Audio.Media.IS_MUSIC} != 0 AND ${MediaStore.Audio.Media.DURATION} >= 15000"
        val entries = mutableListOf<FfiLocalScanEntry>()
        resolver.query(
            collection,
            projection,
            selection,
            null,
            "${MediaStore.Audio.Media.DATE_ADDED} DESC",
        )?.use { cursor ->
            val idIdx = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media._ID)
            val titleIdx = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.TITLE)
            val artistIdx = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.ARTIST)
            val albumIdx = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.ALBUM)
            val durIdx = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.DURATION)
            val mimeIdx = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.MIME_TYPE)
            val nameIdx = cursor.getColumnIndexOrThrow(MediaStore.Audio.Media.DISPLAY_NAME)
            val brIdx = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                cursor.getColumnIndex(MediaStore.Audio.Media.BITRATE)
            } else {
                -1
            }

            while (cursor.moveToNext()) {
                val id = cursor.getLong(idIdx)
                val uri = ContentUris.withAppendedId(collection, id).toString()
                val rawTitle = cursor.getString(titleIdx)?.trim().orEmpty()
                val displayName = cursor.getString(nameIdx)?.trim().orEmpty()
                val title = rawTitle.ifEmpty { displayName.substringBeforeLast('.') }
                val artist = cursor.getString(artistIdx)?.trim()
                    .orEmpty()
                    .takeUnless { it == "<unknown>" }
                    .orEmpty()
                val album = cursor.getString(albumIdx)?.trim()
                    .orEmpty()
                    .takeUnless { it == "<unknown>" }
                    .orEmpty()
                val durationMs = cursor.getLong(durIdx).coerceAtLeast(0L).toULong()
                val mime = cursor.getString(mimeIdx).orEmpty()
                val codec = inferCodec(displayName, mime)
                val bitrateKbps = if (brIdx >= 0) {
                    (cursor.getInt(brIdx) / 1000).coerceAtLeast(0).toUInt()
                } else {
                    0u
                }
                entries.add(
                    FfiLocalScanEntry(
                        uri = uri,
                        title = title.ifEmpty { "Untitled Track" },
                        artist = artist.ifEmpty { "Unknown Artist" },
                        album = album.ifEmpty { "Local Music" },
                        durationMs = durationMs,
                        codec = codec,
                        sampleRate = 44_100u,
                        bitDepth = if (codec == "FLAC" || codec == "WAV" || codec == "ALAC") 24u else 16u,
                        bitrateKbps = bitrateKbps,
                    ),
                )
            }
        }
        return entries
    }

    fun scanSafTree(context: Context, treeUri: Uri, maxFiles: Int = 2_000): List<FfiLocalScanEntry> {
        val root = DocumentFile.fromTreeUri(context, treeUri) ?: return emptyList()
        val out = mutableListOf<FfiLocalScanEntry>()
        val stack = ArrayDeque<DocumentFile>()
        stack.add(root)
        while (stack.isNotEmpty() && out.size < maxFiles) {
            val node = stack.removeFirst()
            for (child in node.listFiles()) {
                if (out.size >= maxFiles) break
                if (child.isDirectory) {
                    stack.addLast(child)
                } else if (child.isFile) {
                    val name = child.name.orEmpty()
                    val ext = name.substringAfterLast('.', "").lowercase(Locale.ROOT)
                    if (ext in SUPPORTED_EXTENSIONS) {
                        out.add(extractSafTrackMetadata(context, child.uri, name, child.type.orEmpty()))
                    }
                }
            }
        }
        return out
    }

    private fun extractSafTrackMetadata(
        context: Context,
        uri: Uri,
        fileName: String,
        mime: String,
    ): FfiLocalScanEntry {
        var title = fileName.substringBeforeLast('.').ifEmpty { "Untitled Track" }
        var artist = "Unknown Artist"
        var album = "Local Folder"
        var durationMs = 0UL
        var bitrateKbps = 0u
        val retriever = MediaMetadataRetriever()
        try {
            retriever.setDataSource(context, uri)
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_TITLE)
                ?.trim()?.takeIf { it.isNotEmpty() }?.let { title = it }
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_ARTIST)
                ?.trim()?.takeIf { it.isNotEmpty() }?.let { artist = it }
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_ALBUM)
                ?.trim()?.takeIf { it.isNotEmpty() }?.let { album = it }
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION)
                ?.toLongOrNull()?.coerceAtLeast(0L)?.let { durationMs = it.toULong() }
            retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_BITRATE)
                ?.toIntOrNull()?.let { bitrateKbps = (it / 1000).coerceAtLeast(0).toUInt() }
        } catch (_: Exception) {
            // Fall back to filename metadata when OEM retriever skips exotic Hi-Res containers;
            // Symphonia in Rust decodes them natively via AndroidContentOpener.
        } finally {
            runCatching { retriever.release() }
        }
        val codec = inferCodec(fileName, mime)
        return FfiLocalScanEntry(
            uri = uri.toString(),
            title = title,
            artist = artist,
            album = album,
            durationMs = durationMs,
            codec = codec,
            sampleRate = 44_100u,
            bitDepth = if (codec == "FLAC" || codec == "WAV" || codec == "ALAC") 24u else 16u,
            bitrateKbps = bitrateKbps,
        )
    }

    private fun inferCodec(fileName: String, mime: String): String {
        val ext = fileName.substringAfterLast('.', "").lowercase(Locale.ROOT)
        if (ext in SUPPORTED_EXTENSIONS) {
            return ext.uppercase(Locale.ROOT)
        }
        val low = mime.lowercase(Locale.ROOT)
        return when {
            "flac" in low -> "FLAC"
            "mpeg" in low || "mp3" in low -> "MP3"
            "mp4" in low || "aac" in low || "m4a" in low -> "M4A"
            "opus" in low -> "OPUS"
            "ogg" in low -> "OGG"
            "wav" in low -> "WAV"
            "aiff" in low -> "AIFF"
            else -> "AUDIO"
        }
    }
}
