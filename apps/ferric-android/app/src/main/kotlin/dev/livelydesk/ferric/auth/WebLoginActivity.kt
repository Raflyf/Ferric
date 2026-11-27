package dev.livelydesk.ferric.auth

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature
import dev.livelydesk.ferric.FerricApp
import dev.livelydesk.ferric.ui.theme.AmoledBlack
import dev.livelydesk.ferric.ui.theme.FerricOrange
import dev.livelydesk.ferric.ui.theme.FerricTheme
import dev.livelydesk.ferric.ui.theme.ObsidianSurface
import dev.livelydesk.ferric.ui.theme.TextPrimary
import dev.livelydesk.ferric.ui.theme.TextSecondary
import java.util.concurrent.atomic.AtomicBoolean

object WebCookieCleaner {
    fun clearAllWebAuthData() {
        runCatching {
            CookieManager.getInstance().removeAllCookies(null)
            CookieManager.getInstance().flush()
            WebStorage.getInstance().deleteAllData()
        }
    }
}

/**
 * Isolated app-private WebView Activity strictly for optional Spotify and YouTube Music sign-in.
 * Enforces [LoginHostAllowlist], disables file/content access, strips `; wv` for Google OAuth,
 * and captures session cookies/bearer tokens without exposing any `@JavascriptInterface`.
 */
class WebLoginActivity : ComponentActivity() {
    private val mainHandler = Handler(Looper.getMainLooper())
    private val completed = AtomicBoolean(false)
    private var webViewRef: WebView? = null
    private var isLoadingState by mutableStateOf(true)
    private var statusHint by mutableStateOf("Sign in to link your account")
    private lateinit var target: WebLoginTarget

    private val pollRunnable = object : Runnable {
        override fun run() {
            if (completed.get() || isFinishing) return
            pollSessionCredentials()
            if (!completed.get() && !isFinishing) {
                mainHandler.postDelayed(this, POLL_INTERVAL_MS)
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.BLACK),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.BLACK),
        )
        val rawTarget = intent.getStringExtra(EXTRA_TARGET)
        target = if (rawTarget == WebLoginTarget.SPOTIFY.name) {
            WebLoginTarget.SPOTIFY
        } else {
            WebLoginTarget.YOUTUBE_MUSIC
        }

        val title = when (target) {
            WebLoginTarget.SPOTIFY -> "Sign in to Spotify"
            WebLoginTarget.YOUTUBE_MUSIC -> "Sign in to YouTube Music"
        }

        setContent {
            FerricTheme {
                Column(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(AmoledBlack)
                        .statusBarsPadding(),
                ) {
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .background(ObsidianSurface)
                            .padding(horizontal = 8.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        IconButton(onClick = { finish() }) {
                            Icon(
                                imageVector = Icons.Rounded.Close,
                                contentDescription = "Close sign in",
                                tint = TextPrimary,
                            )
                        }
                        Spacer(Modifier.width(4.dp))
                        Column(modifier = Modifier.weight(1f)) {
                            Text(title, style = MaterialTheme.typography.titleMedium, color = TextPrimary)
                            Text(statusHint, style = MaterialTheme.typography.labelSmall, color = TextSecondary)
                        }
                    }
                    if (isLoadingState) {
                        LinearProgressIndicator(
                            modifier = Modifier.fillMaxWidth(),
                            color = FerricOrange,
                            trackColor = ObsidianSurface,
                        )
                    }
                    AndroidView(
                        modifier = Modifier
                            .fillMaxWidth()
                            .weight(1f),
                        factory = { ctx -> createHardenedWebView(ctx) },
                    )
                }
            }
        }

        mainHandler.postDelayed(pollRunnable, POLL_INTERVAL_MS)
    }

    @SuppressLint("SetJavaScriptEnabled")
    private fun createHardenedWebView(context: Context): WebView {
        val wv = WebView(context).apply {
            layoutParams = ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            )
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.allowFileAccess = false
            settings.allowContentAccess = false
            settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
            settings.setSupportMultipleWindows(false)
            // Strip Android WebView marker so Google login does not return 403 disallowed_useragent.
            settings.userAgentString = settings.userAgentString
                .replace("; wv", "")
                .replace("Version/4.0 ", "")
        }

        val cm = CookieManager.getInstance()
        cm.setAcceptCookie(true)
        cm.setAcceptThirdPartyCookies(wv, true)

        if (target == WebLoginTarget.SPOTIFY &&
            WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)
        ) {
            WebViewCompat.addDocumentStartJavaScript(
                wv,
                LoginHostAllowlist.SPOTIFY_TOKEN_HOOK_JS,
                setOf(LoginHostAllowlist.SPOTIFY_ORIGIN),
            )
        }

        wv.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(
                view: WebView?,
                request: WebResourceRequest?,
            ): Boolean {
                val url = request?.url?.toString() ?: return true
                return !LoginHostAllowlist.isAllowedNavigationUrl(target, url)
            }

            override fun onPageStarted(view: WebView?, url: String?, favicon: Bitmap?) {
                isLoadingState = true
                if (target == WebLoginTarget.SPOTIFY &&
                    url?.startsWith(LoginHostAllowlist.SPOTIFY_ORIGIN) == true
                ) {
                    view?.evaluateJavascript(LoginHostAllowlist.SPOTIFY_TOKEN_HOOK_JS, null)
                }
            }

            override fun onPageFinished(view: WebView?, url: String?) {
                isLoadingState = false
                if (target == WebLoginTarget.SPOTIFY &&
                    url?.startsWith(LoginHostAllowlist.SPOTIFY_ORIGIN) == true
                ) {
                    view?.evaluateJavascript(LoginHostAllowlist.SPOTIFY_TOKEN_HOOK_JS, null)
                }
                pollSessionCredentials()
            }
        }

        val startUrl = when (target) {
            WebLoginTarget.SPOTIFY -> LoginHostAllowlist.SPOTIFY_LOGIN_URL
            WebLoginTarget.YOUTUBE_MUSIC -> LoginHostAllowlist.GOOGLE_YTM_LOGIN_URL
        }
        wv.loadUrl(startUrl)
        webViewRef = wv
        return wv
    }

    private fun pollSessionCredentials() {
        val wv = webViewRef ?: return
        val currentUrl = wv.url.orEmpty()
        val cm = CookieManager.getInstance()
        when (target) {
            WebLoginTarget.YOUTUBE_MUSIC -> {
                if (!currentUrl.startsWith(LoginHostAllowlist.YTM_ORIGIN)) return
                val cookie = cm.getCookie(LoginHostAllowlist.YTM_ORIGIN).orEmpty()
                val sapisid = LoginHostAllowlist.extractSapisid(cookie) ?: return
                if (completed.compareAndSet(false, true)) {
                    cm.flush()
                    statusHint = "Connected! Syncing YouTube Music..."
                    (application as FerricApp).store.completeYtmLogin(
                        cookie = cookie,
                        sapisid = sapisid,
                    )
                    finish()
                }
            }
            WebLoginTarget.SPOTIFY -> {
                if (!currentUrl.startsWith(LoginHostAllowlist.SPOTIFY_ORIGIN)) return
                val cookie = cm.getCookie(LoginHostAllowlist.SPOTIFY_ORIGIN).orEmpty()
                val spDc = LoginHostAllowlist.extractSpDc(cookie) ?: return
                wv.evaluateJavascript("window.__ferricToken || null") { raw ->
                    val token = LoginHostAllowlist.parseAccessTokenFromJson(raw)
                    if (token != null && completed.compareAndSet(false, true)) {
                        cm.flush()
                        statusHint = "Connected! Syncing Spotify..."
                        (application as FerricApp).store.completeSpotifyLogin(
                            accessToken = token,
                            cookie = "sp_dc=$spDc",
                            triggerSync = true,
                        )
                        finish()
                    }
                }
            }
        }
    }

    override fun onDestroy() {
        mainHandler.removeCallbacks(pollRunnable)
        webViewRef?.apply {
            stopLoading()
            destroy()
        }
        webViewRef = null
        super.onDestroy()
    }

    companion object {
        private const val EXTRA_TARGET = "ferric_web_login_target"
        private const val POLL_INTERVAL_MS = 1_200L

        fun createIntent(context: Context, target: WebLoginTarget): Intent =
            Intent(context, WebLoginActivity::class.java).putExtra(EXTRA_TARGET, target.name)
    }
}
