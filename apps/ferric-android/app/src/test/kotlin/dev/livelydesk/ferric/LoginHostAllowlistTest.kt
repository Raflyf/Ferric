package dev.livelydesk.ferric

import dev.livelydesk.ferric.auth.LoginHostAllowlist
import dev.livelydesk.ferric.auth.WebLoginTarget
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class LoginHostAllowlistTest {
    @Test
    fun permitsLegitimateAuthDomainsAndBlocksEvilOrLocalSchemes() {
        assertTrue(
            LoginHostAllowlist.isAllowedNavigationUrl(
                WebLoginTarget.SPOTIFY,
                "https://accounts.spotify.com/en/login",
            ),
        )
        assertTrue(
            LoginHostAllowlist.isAllowedNavigationUrl(
                WebLoginTarget.SPOTIFY,
                "https://open.spotify.com/",
            ),
        )
        assertTrue(
            LoginHostAllowlist.isAllowedNavigationUrl(
                WebLoginTarget.YOUTUBE_MUSIC,
                "https://accounts.google.com/ServiceLogin",
            ),
        )
        assertTrue(
            LoginHostAllowlist.isAllowedNavigationUrl(
                WebLoginTarget.YOUTUBE_MUSIC,
                "https://music.youtube.com/",
            ),
        )

        // Domain suffix confusion, plaintext HTTP, file://, raw IPs, and userinfo must fail closed.
        assertFalse(
            LoginHostAllowlist.isAllowedNavigationUrl(
                WebLoginTarget.SPOTIFY,
                "https://evilspotify.com/login",
            ),
        )
        assertFalse(
            LoginHostAllowlist.isAllowedNavigationUrl(
                WebLoginTarget.SPOTIFY,
                "http://open.spotify.com/",
            ),
        )
        assertFalse(
            LoginHostAllowlist.isAllowedNavigationUrl(
                WebLoginTarget.YOUTUBE_MUSIC,
                "file:///data/local/tmp/leak.html",
            ),
        )
        assertFalse(
            LoginHostAllowlist.isAllowedNavigationUrl(
                WebLoginTarget.YOUTUBE_MUSIC,
                "https://127.0.0.1/callback",
            ),
        )
        assertFalse(
            LoginHostAllowlist.isAllowedNavigationUrl(
                WebLoginTarget.SPOTIFY,
                "https://accounts.spotify.com@evil.example/phish",
            ),
        )
    }

    @Test
    fun extractsCookiesAndParsesNonAnonymousSpotifyToken() {
        val cookie = "SID=abc; __Secure-3PAPISID=secret_sapi_123; sp_dc=sp_cookie_xyz"
        assertEquals("secret_sapi_123", LoginHostAllowlist.extractSapisid(cookie))
        assertEquals("sp_cookie_xyz", LoginHostAllowlist.extractSpDc(cookie))

        val validJs = "\"{\\\"accessToken\\\":\\\"BQD_valid_token_string_1234567890\\\",\\\"isAnonymous\\\":false}\""
        assertEquals(
            "BQD_valid_token_string_1234567890",
            LoginHostAllowlist.parseAccessTokenFromJson(validJs),
        )

        val anonJs = "\"{\\\"accessToken\\\":\\\"BQD_anonymous_token_1234567890\\\",\\\"isAnonymous\\\":true}\""
        assertNull(LoginHostAllowlist.parseAccessTokenFromJson(anonJs))
    }
}
