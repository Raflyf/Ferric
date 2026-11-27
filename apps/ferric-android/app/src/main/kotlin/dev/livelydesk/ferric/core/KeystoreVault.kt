package dev.livelydesk.ferric.core

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import uniffi.ferric_mobile_ffi.FfiException
import uniffi.ferric_mobile_ffi.FfiSecretCipher

/**
 * Pure framing helper for AES-256-GCM payloads (`12-byte IV || ciphertext+16-byte-tag`)
 * kept free of Android framework dependencies so JVM unit tests exercise it directly.
 */
object KeystoreEnvelopeCodec {
    const val IV_SIZE_BYTES: Int = 12
    const val GCM_TAG_BITS: Int = 128
    const val MIN_CIPHER_BYTES: Int = IV_SIZE_BYTES + (GCM_TAG_BITS / 8)

    fun pack(iv: ByteArray, ciphertextWithTag: ByteArray): ByteArray {
        require(iv.size == IV_SIZE_BYTES) { "GCM IV must be $IV_SIZE_BYTES bytes" }
        require(ciphertextWithTag.size >= GCM_TAG_BITS / 8) { "GCM ciphertext missing auth tag" }
        val out = ByteArray(iv.size + ciphertextWithTag.size)
        iv.copyInto(out, destinationOffset = 0)
        ciphertextWithTag.copyInto(out, destinationOffset = iv.size)
        return out
    }

    fun unpack(payload: ByteArray): Pair<ByteArray, ByteArray> {
        require(payload.size >= MIN_CIPHER_BYTES) { "Keystore envelope truncated" }
        val iv = payload.copyOfRange(0, IV_SIZE_BYTES)
        val ciphertext = payload.copyOfRange(IV_SIZE_BYTES, payload.size)
        return iv to ciphertext
    }
}

/**
 * Hardware-backed Android Keystore AES-256-GCM secret cipher implementing Rust's [FfiSecretCipher].
 * Fails closed by throwing [FfiException.SecretProtection] on any Keystore error.
 */
class KeystoreVault(
    private val keyAlias: String = "ferric_accounts_master_v1",
) : FfiSecretCipher {
    private val lock = Any()

    private fun getOrCreateMasterKey(): SecretKey {
        val ks = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val existing = ks.getKey(keyAlias, null) as? SecretKey
        if (existing != null) return existing

        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        val spec = KeyGenParameterSpec.Builder(
            keyAlias,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setRandomizedEncryptionRequired(true)
            .build()
        generator.init(spec)
        return generator.generateKey()
    }

    override fun encrypt(plain: ByteArray): ByteArray = synchronized(lock) {
        try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.ENCRYPT_MODE, getOrCreateMasterKey())
            val iv = cipher.iv
            val ct = cipher.doFinal(plain)
            KeystoreEnvelopeCodec.pack(iv, ct)
        } catch (err: Exception) {
            throw FfiException.SecretProtection("Android Keystore encrypt failed: ${err.message}")
        }
    }

    override fun decrypt(cipher: ByteArray): ByteArray = synchronized(lock) {
        try {
            val (iv, body) = KeystoreEnvelopeCodec.unpack(cipher)
            val engine = Cipher.getInstance(TRANSFORMATION)
            engine.init(
                Cipher.DECRYPT_MODE,
                getOrCreateMasterKey(),
                GCMParameterSpec(KeystoreEnvelopeCodec.GCM_TAG_BITS, iv),
            )
            engine.doFinal(body)
        } catch (err: Exception) {
            throw FfiException.SecretProtection("Android Keystore decrypt failed: ${err.message}")
        }
    }

    private companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
    }
}
