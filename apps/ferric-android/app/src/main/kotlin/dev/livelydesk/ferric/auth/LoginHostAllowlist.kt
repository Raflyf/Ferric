package dev.livelydesk.ferric.auth

import java.net.URI

enum class WebLoginTarget {
    SPOTIFY,
    YOUTUBE_MUSIC,
}

/**
 * Pure JVM-testable URL & domain allowlist for isolated WebView login and silent token refresh.
 * Mirrors Ferric Desktop `google_login.rs` fail-closed navigation controls.
 */
object LoginHostAllowlist {
    const val GOOGLE_YTM_LOGIN_URL: String =
        "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube&passive=true&continue=https%3A%2F%2Fwww.youtube.com%2Fsignin%3Faction_handle_signin%3Dtrue%26next%3Dhttps%253A%252F%252Fmusic.youtube.com%252F"
    const val YTM_ORIGIN: String = "https://music.youtube.com"
    const val SPOTIFY_LOGIN_URL: String =
        "https://accounts.spotify.com/en/login?continue=https%3A%2F%2Fopen.spotify.com%2F"
    const val SPOTIFY_ORIGIN: String = "https://open.spotify.com"

    private val GOOGLE_ALLOWED_DOMAINS = listOf(
        "google.com",
        "youtube.com",
        "gstatic.com",
        "googleapis.com",
        "googleusercontent.com",
        "recaptcha.net",
    )

    private val SPOTIFY_ALLOWED_DOMAINS = listOf(
        "spotify.com",
        "spotifycdn.com",
        "scdn.co",
        "google.com",
        "gstatic.com",
        "googleapis.com",
        "apple.com",
        "facebook.com",
        "fbcdn.net",
        "recaptcha.net",
        "hcaptcha.com",
        "arkoselabs.com",
    )

    fun hostMatchesDomain(host: String, domain: String): Boolean {
        val h = host.lowercase()
        val d = domain.lowercase()
        return h == d || (h.length > d.length && h.endsWith(".$d"))
    }

    private fun isRawIpLiteral(host: String): Boolean {
        if (host.contains(':')) return true
        val parts = host.split('.')
        return parts.isNotEmpty() && parts.all { p -> p.isNotEmpty() && p.all { it.isDigit() } }
    }

    fun isAllowedNavigationUrl(target: WebLoginTarget, rawUrl: String?): Boolean {
        val url = rawUrl?.trim() ?: return false
        if (url.isEmpty()) return false
        if (url.equals("about:blank", ignoreCase = true)) return true
        if (url.any { it.code < 0x20 || it == ' ' || it == '\\' }) return false
        if (!url.startsWith("https://", ignoreCase = true)) return false

        val uri = runCatching { URI(url) }.getOrNull() ?: return false
        if (!uri.scheme.equals("https", ignoreCase = true)) return false
        if (!uri.userInfo.isNullOrEmpty()) return false
        val host = uri.host?.lowercase()?.trimEnd('.') ?: return false
        if (host.isEmpty() || isRawIpLiteral(host)) return false

        val allowed = when (target) {
            WebLoginTarget.YOUTUBE_MUSIC -> GOOGLE_ALLOWED_DOMAINS
            WebLoginTarget.SPOTIFY -> SPOTIFY_ALLOWED_DOMAINS
        }
        return allowed.any { hostMatchesDomain(host, it) }
    }

    fun extractCookieValue(cookieHeader: String?, name: String): String? {
        if (cookieHeader.isNullOrBlank()) return null
        val target = name.trim()
        for (part in cookieHeader.split(';')) {
            val eq = part.indexOf('=')
            if (eq <= 0) continue
            val k = part.substring(0, eq).trim()
            if (k == target) {
                val v = part.substring(eq + 1).trim()
                if (v.isNotEmpty()) return v
            }
        }
        return null
    }

    fun extractSapisid(cookieHeader: String?): String? =
        extractCookieValue(cookieHeader, "__Secure-3PAPISID")
            ?: extractCookieValue(cookieHeader, "SAPISID")
            ?: extractCookieValue(cookieHeader, "__Secure-1PAPISID")

    fun extractSpDc(cookieHeader: String?): String? =
        extractCookieValue(cookieHeader, "sp_dc")

    fun parseAccessTokenFromJson(rawJsResult: String?): String? {
        if (rawJsResult.isNullOrBlank()) return null
        val trimmed = rawJsResult.trim()
        if (trimmed == "null" || trimmed.isEmpty()) return null
        // evaluateJavascript returns a JSON-encoded string literal; unquote cleanly.
        val unescaped = if (trimmed.startsWith("\"") && trimmed.endsWith("\"") && trimmed.length >= 2) {
            trimmed.substring(1, trimmed.length - 1)
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
        } else {
            trimmed
        }
        if (unescaped.contains("\"isAnonymous\":true") || unescaped.contains("\"isAnonymous\": true")) {
            return null
        }
        val match = Regex("\"accessToken\"\\s*:\\s*\"([^\"]{20,})\"").find(unescaped) ?: return null
        return match.groupValues[1]
    }

    const val SPOTIFY_TOKEN_HOOK_JS: String = """
(() => {
  if (window.__ferricTokenHook) return;
  window.__ferricTokenHook = true;
  window.__ferricToken = null;
  const isTokenUrl = (u) => {
    try { return /(\/api\/token|get_access_token|\/token)/.test(String(u)); } catch (e) { return false; }
  };
  const onPlayer = () => {
    try { return location.hostname === 'open.spotify.com'; } catch (e) { return false; }
  };
  const setToken = (text) => {
    if (!onPlayer() || typeof text !== 'string' || text.length > 65536) return;
    if (text.indexOf('accessToken') === -1) return;
    try {
      const parsed = JSON.parse(text);
      if (!parsed || !parsed.accessToken || parsed.isAnonymous === true) return;
      window.__ferricToken = text;
    } catch (e) {}
  };
  const setBearer = (value) => {
    if (!onPlayer() || typeof value !== 'string') return;
    const match = /^Bearer\s+(.+)$/i.exec(value.trim());
    if (!match) return;
    const token = match[1].trim();
    if (token.length < 20) return;
    if (!window.__ferricToken) {
      window.__ferricToken = JSON.stringify({ accessToken: token, isAnonymous: false, source: 'bearer' });
    }
  };
  const readHeaders = (headers) => {
    try {
      if (!headers) return;
      if (typeof headers.get === 'function') { setBearer(headers.get('authorization')); return; }
      if (Array.isArray(headers)) {
        headers.forEach((pair) => {
          if (pair && String(pair[0]).toLowerCase() === 'authorization') setBearer(pair[1]);
        });
        return;
      }
      Object.keys(headers).forEach((k) => {
        if (k.toLowerCase() === 'authorization') setBearer(headers[k]);
      });
    } catch (e) {}
  };
  const originalFetch = window.fetch;
  if (originalFetch) {
    window.fetch = function (input, init) {
      try {
        if (input && typeof input === 'object' && input.headers) readHeaders(input.headers);
        if (init && init.headers) readHeaders(init.headers);
      } catch (e) {}
      const url = (input && input.url) || input;
      const pending = originalFetch.apply(this, arguments);
      if (isTokenUrl(url)) {
        pending.then((res) => res.clone().text()).then(setToken).catch(() => {});
      }
      return pending;
    };
  }
  const originalSetHeader = XMLHttpRequest.prototype.setRequestHeader;
  XMLHttpRequest.prototype.setRequestHeader = function (name, value) {
    if (String(name).toLowerCase() === 'authorization') setBearer(value);
    return originalSetHeader.apply(this, arguments);
  };
  const originalOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url) {
    if (isTokenUrl(url)) {
      this.addEventListener('load', () => {
        try { setToken(this.responseText); } catch (e) {}
      });
    }
    return originalOpen.apply(this, arguments);
  };
})();
"""
}
