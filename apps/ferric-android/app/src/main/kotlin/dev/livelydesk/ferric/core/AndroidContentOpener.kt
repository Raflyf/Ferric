package dev.livelydesk.ferric.core

import android.content.ContentResolver
import androidx.core.net.toUri
import uniffi.ferric_mobile_ffi.FfiContentOpener
import uniffi.ferric_mobile_ffi.FfiException

/**
 * Android Scoped Storage bridge allowing Rust `ferric-audio` to decode `content://` URIs
 * (MediaStore and SAF folders) via read-only detached POSIX file descriptors without requesting
 * `MANAGE_EXTERNAL_STORAGE`.
 */
class AndroidContentOpener(
    private val contentResolver: ContentResolver,
) : FfiContentOpener {
    override fun openFd(uri: String): Int {
        if (!uri.startsWith("content://")) {
            throw FfiException.Io("Rejected non-content URI in AndroidContentOpener")
        }
        return try {
            val pfd = contentResolver.openFileDescriptor(uri.toUri(), "r")
                ?: throw FfiException.Io("ContentResolver returned null descriptor")
            pfd.detachFd()
        } catch (err: FfiException) {
            throw err
        } catch (err: Exception) {
            throw FfiException.Io("Failed to open content URI: ${err.message}")
        }
    }
}
