package dev.livelydesk.ferric.auth

import android.annotation.SuppressLint
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Silent offscreen WebView refresher invoked automatically when Rust reports Spotify's
 * 1-hour web-player bearer token expired (`onSpotifyAuthExpired`), reusing the app-private
 * `CookieManager` `sp_dc` cookie so users are never repeatedly prompted to sign in.
 */
class SilentSpotifyRefresher(
    private val appContext: Context,
    private val onRefreshed: (accessToken: String, cookie: String) -> Unit,
    private val onFailed: () -> Unit,
) {
    private val mainHandler = Handler(Looper.getMainLooper())
    private val inFlight = AtomicBoolean(false)

    @SuppressLint("SetJavaScriptEnabled")
    fun triggerRefresh() {
        if (!inFlight.compareAndSet(false, true)) return
        mainHandler.post {
            val cm = CookieManager.getInstance()
            val cookie = cm.getCookie(LoginHostAllowlist.SPOTIFY_ORIGIN).orEmpty()
            val spDc = LoginHostAllowlist.extractSpDc(cookie)
            if (spDc.isNullOrEmpty()) {
                inFlight.set(false)
                onFailed()
                return@post
            }

            val finished = AtomicBoolean(false)
            val wv = WebView(appContext).apply {
                settings.javaScriptEnabled = true
                settings.domStorageEnabled = true
                settings.allowFileAccess = false
                settings.allowContentAccess = false
                settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
                settings.setSupportMultipleWindows(false)
            }
            cm.setAcceptCookie(true)
            cm.setAcceptThirdPartyCookies(wv, true)

            if (WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) {
                WebViewCompat.addDocumentStartJavaScript(
                    wv,
                    LoginHostAllowlist.SPOTIFY_TOKEN_HOOK_JS,
                    setOf(LoginHostAllowlist.SPOTIFY_ORIGIN),
                )
            }

            fun teardown() {
                mainHandler.post {
                    runCatching {
                        wv.stopLoading()
                        wv.destroy()
                    }
                    inFlight.set(false)
                }
            }

            val timeoutRunnable = Runnable {
                if (finished.compareAndSet(false, true)) {
                    teardown()
                    onFailed()
                }
            }

            val pollTask = object : Runnable {
                override fun run() {
                    if (finished.get()) return
                    wv.evaluateJavascript("window.__ferricToken || null") { raw ->
                        val token = LoginHostAllowlist.parseAccessTokenFromJson(raw)
                        if (token != null && finished.compareAndSet(false, true)) {
                            mainHandler.removeCallbacks(timeoutRunnable)
                            val latestCookie = cm.getCookie(LoginHostAllowlist.SPOTIFY_ORIGIN).orEmpty()
                            val latestSpDc = LoginHostAllowlist.extractSpDc(latestCookie) ?: spDc
                            teardown()
                            onRefreshed(token, "sp_dc=$latestSpDc")
                        } else if (!finished.get()) {
                            mainHandler.postDelayed(this, 1_000L)
                        }
                    }
                }
            }

            wv.webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(
                    view: WebView?,
                    request: WebResourceRequest?,
                ): Boolean {
                    val url = request?.url?.toString() ?: return true
                    return !LoginHostAllowlist.isAllowedNavigationUrl(WebLoginTarget.SPOTIFY, url)
                }

                override fun onPageFinished(view: WebView?, url: String?) {
                    if (url?.startsWith(LoginHostAllowlist.SPOTIFY_ORIGIN) == true) {
                        view?.evaluateJavascript(LoginHostAllowlist.SPOTIFY_TOKEN_HOOK_JS, null)
                    }
                }
            }

            mainHandler.postDelayed(timeoutRunnable, 15_000L)
            mainHandler.postDelayed(pollTask, 1_500L)
            wv.loadUrl("${LoginHostAllowlist.SPOTIFY_ORIGIN}/")
        }
    }
}
