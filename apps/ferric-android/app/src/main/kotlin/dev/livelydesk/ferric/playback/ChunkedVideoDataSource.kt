package dev.livelydesk.ferric.playback

import android.net.Uri
import androidx.annotation.OptIn
import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.BaseDataSource
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.DefaultHttpDataSource
import java.io.IOException
import kotlin.math.min

private const val VISIONOS_STREAM_USER_AGENT =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15"
private const val VIDEO_RANGE_CHUNK_BYTES = 2L * 1024L * 1024L

/**
 * Streaming HTTP [DataSource] that slices large or open-ended progressive CDN requests into
 * bounded 2 MiB HTTP 206 byte-range chunks with the matching upstream client User-Agent.
 * Prevents single-connection CDN bandwidth throttling that stalls 720p/1080p video streams while
 * keeping low latency and zero temp-file disk overhead.
 */
@OptIn(UnstableApi::class)
internal class ChunkedVideoDataSource(
    private val upstream: DefaultHttpDataSource,
) : BaseDataSource(/* isNetwork = */ true) {

    class Factory : DataSource.Factory {
        private val upstreamFactory = DefaultHttpDataSource.Factory()
            .setUserAgent(VISIONOS_STREAM_USER_AGENT)
            .setAllowCrossProtocolRedirects(true)
            .setConnectTimeoutMs(10_000)
            .setReadTimeoutMs(15_000)

        override fun createDataSource(): DataSource =
            ChunkedVideoDataSource(upstreamFactory.createDataSource())
    }

    private var rootSpec: DataSpec? = null
    private var currentUri: Uri? = null
    private var opened = false
    private var upstreamOpened = false
    private var chunkingActive = false
    private var nextPosition = 0L
    private var totalBytesRemainingToCaller = C.LENGTH_UNSET.toLong()
    private var knownTotalResourceSize = C.LENGTH_UNSET.toLong()
    private var currentChunkBytesRemaining = C.LENGTH_UNSET.toLong()
    private var lastResponseHeaders: Map<String, List<String>> = emptyMap()

    @Throws(IOException::class)
    override fun open(dataSpec: DataSpec): Long {
        rootSpec = dataSpec
        currentUri = dataSpec.uri
        nextPosition = dataSpec.position.coerceAtLeast(0L)
        totalBytesRemainingToCaller = dataSpec.length
        knownTotalResourceSize = C.LENGTH_UNSET.toLong()
        chunkingActive = false

        transferInitializing(dataSpec)
        val initialOpenedBytes = openNextChunk(dataSpec)
        opened = true
        transferStarted(dataSpec)
        return initialOpenedBytes
    }

    @Throws(IOException::class)
    private fun openNextChunk(baseSpec: DataSpec): Long {
        closeUpstreamQuietly()

        val requestedToCaller = totalBytesRemainingToCaller
        val boundedRemaining = if (knownTotalResourceSize != C.LENGTH_UNSET.toLong()) {
            (knownTotalResourceSize - nextPosition).coerceAtLeast(0L)
        } else {
            Long.MAX_VALUE
        }

        val chunkLen = when {
            requestedToCaller != C.LENGTH_UNSET.toLong() ->
                min(requestedToCaller, VIDEO_RANGE_CHUNK_BYTES).coerceAtMost(boundedRemaining)
            boundedRemaining != Long.MAX_VALUE ->
                min(boundedRemaining, VIDEO_RANGE_CHUNK_BYTES)
            else -> VIDEO_RANGE_CHUNK_BYTES
        }
        if (chunkLen <= 0L) {
            currentChunkBytesRemaining = 0L
            return 0L
        }

        val chunkSpec = baseSpec.buildUpon()
            .setPosition(nextPosition)
            .setLength(chunkLen)
            .build()

        val upstreamLen = upstream.open(chunkSpec)
        upstreamOpened = true
        currentUri = upstream.uri ?: baseSpec.uri
        lastResponseHeaders = upstream.responseHeaders

        val contentRangeHeader = lastResponseHeaders.entries
            .firstOrNull { it.key.equals("Content-Range", ignoreCase = true) }
            ?.value
            ?.firstOrNull()
        val parsedTotal = parseContentRangeTotal(contentRangeHeader)
        if (parsedTotal > 0L) {
            knownTotalResourceSize = parsedTotal
        }

        chunkingActive = contentRangeHeader != null
        currentChunkBytesRemaining = upstreamLen

        return if (baseSpec.length != C.LENGTH_UNSET.toLong()) {
            if (knownTotalResourceSize != C.LENGTH_UNSET.toLong()) {
                min(baseSpec.length, (knownTotalResourceSize - baseSpec.position).coerceAtLeast(0L))
            } else {
                baseSpec.length
            }
        } else if (knownTotalResourceSize != C.LENGTH_UNSET.toLong()) {
            (knownTotalResourceSize - baseSpec.position).coerceAtLeast(0L)
        } else {
            upstreamLen
        }
    }

    @Throws(IOException::class)
    override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
        if (length == 0) return 0
        if (totalBytesRemainingToCaller == 0L) return C.RESULT_END_OF_INPUT

        val spec = rootSpec ?: return C.RESULT_END_OF_INPUT
        val maxRead = if (totalBytesRemainingToCaller != C.LENGTH_UNSET.toLong()) {
            min(length.toLong(), totalBytesRemainingToCaller).toInt()
        } else {
            length
        }

        while (true) {
            if (!upstreamOpened || (currentChunkBytesRemaining == 0L && chunkingActive)) {
                if (!hasMoreBytesToFetch()) {
                    return C.RESULT_END_OF_INPUT
                }
                openNextChunk(spec)
                if (currentChunkBytesRemaining == 0L) {
                    return C.RESULT_END_OF_INPUT
                }
            }

            val bytesRead = upstream.read(buffer, offset, maxRead)
            if (bytesRead == C.RESULT_END_OF_INPUT) {
                if (chunkingActive && hasMoreBytesToFetch()) {
                    closeUpstreamQuietly()
                    continue
                }
                return C.RESULT_END_OF_INPUT
            }

            nextPosition += bytesRead
            if (totalBytesRemainingToCaller != C.LENGTH_UNSET.toLong()) {
                totalBytesRemainingToCaller = (totalBytesRemainingToCaller - bytesRead).coerceAtLeast(0L)
            }
            if (currentChunkBytesRemaining != C.LENGTH_UNSET.toLong()) {
                currentChunkBytesRemaining = (currentChunkBytesRemaining - bytesRead).coerceAtLeast(0L)
            }
            bytesTransferred(bytesRead)
            return bytesRead
        }
    }

    private fun hasMoreBytesToFetch(): Boolean {
        if (totalBytesRemainingToCaller == 0L) return false
        if (knownTotalResourceSize != C.LENGTH_UNSET.toLong() && nextPosition >= knownTotalResourceSize) {
            return false
        }
        return true
    }

    override fun getUri(): Uri? = currentUri

    override fun getResponseHeaders(): Map<String, List<String>> = lastResponseHeaders

    @Throws(IOException::class)
    override fun close() {
        rootSpec = null
        currentUri = null
        try {
            closeUpstreamQuietly()
        } finally {
            if (opened) {
                opened = false
                transferEnded()
            }
        }
    }

    private fun closeUpstreamQuietly() {
        if (upstreamOpened) {
            upstreamOpened = false
            runCatching { upstream.close() }
        }
    }

    private fun parseContentRangeTotal(header: String?): Long {
        if (header.isNullOrBlank()) return C.LENGTH_UNSET.toLong()
        val slash = header.indexOf('/')
        if (slash < 0 || slash + 1 >= header.length) return C.LENGTH_UNSET.toLong()
        val totalPart = header.substring(slash + 1).trim()
        if (totalPart == "*") return C.LENGTH_UNSET.toLong()
        return totalPart.toLongOrNull()?.takeIf { it > 0L } ?: C.LENGTH_UNSET.toLong()
    }
}
