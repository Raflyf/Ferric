package dev.livelydesk.ferric

import dev.livelydesk.ferric.core.KeystoreEnvelopeCodec
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Test

class KeystoreEnvelopeTest {
    @Test
    fun packsAndUnpacksIvAndCiphertextCleanly() {
        val iv = ByteArray(12) { idx -> (idx + 1).toByte() }
        val ciphertext = ByteArray(24) { idx -> (0xA0 + idx).toByte() }

        val packed = KeystoreEnvelopeCodec.pack(iv, ciphertext)
        assertEquals(36, packed.size)

        val (unpackedIv, unpackedCt) = KeystoreEnvelopeCodec.unpack(packed)
        assertArrayEquals(iv, unpackedIv)
        assertArrayEquals(ciphertext, unpackedCt)
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsTruncatedPayloadFailClosed() {
        KeystoreEnvelopeCodec.unpack(ByteArray(10))
    }

    @Test(expected = IllegalArgumentException::class)
    fun rejectsInvalidIvLength() {
        KeystoreEnvelopeCodec.pack(ByteArray(8), ByteArray(24))
    }
}
