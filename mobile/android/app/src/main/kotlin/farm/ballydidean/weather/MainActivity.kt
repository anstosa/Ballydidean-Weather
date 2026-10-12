package farm.ballydidean.weather

import android.annotation.SuppressLint
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.Insets
import android.net.Uri
import android.net.http.SslError
import android.os.Build
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.webkit.CookieManager
import android.webkit.RenderProcessGoneDetail
import android.webkit.SslErrorHandler
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Button
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import farm.ballydidean.weather.widget.WidgetController
import farm.ballydidean.weather.widget.WidgetWorkScheduler
import java.time.Instant

class MainActivity : Activity() {
    private lateinit var webView: WebView
    private lateinit var webContainer: FrameLayout
    private lateinit var errorView: View
    private lateinit var canonicalOrigin: String
    private lateinit var retryUrl: String
    private var mainFrameFailed = false

    // configure the persistent hosted shell
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        canonicalOrigin = HostedTestConfiguration.canonicalOrigin(
            this,
            intent,
            getString(R.string.canonical_weather_origin),
        )
        retryUrl = canonicalOrigin
        webView = createWebView()
        setContentView(shellView())
        // restore navigation without duplicating history
        if (savedInstanceState == null || webView.restoreState(savedInstanceState) == null) {
            webView.loadUrl(startUrl(intent))
        }
        recomputeWidgets()
        // register predictive back on modern android
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            onBackInvokedDispatcher.registerOnBackInvokedCallback(
                android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT,
                ::navigateBack,
            )
        }
    }

    // route a new immutable widget intent
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        webView.loadUrl(startUrl(intent))
        recomputeWidgets()
    }

    // preserve web history across recreation
    override fun onSaveInstanceState(outState: Bundle) {
        webView.saveState(outState)
        super.onSaveInstanceState(outState)
    }

    // preserve in-site browser history
    @Deprecated("legacy fallback below api 33")
    @Suppress("DEPRECATION")
    @SuppressLint("GestureBackNavigation")
    override fun onBackPressed() {
        navigateBack()
    }

    // preserve in-site browser history
    private fun navigateBack() {
        // return within the hosted app
        if (webView.canGoBack()) {
            webView.goBack()
            return
        }
        finishAfterTransition()
    }

    // release native browser resources
    override fun onDestroy() {
        webView.destroy()
        super.onDestroy()
    }

    // construct the retry shell
    private fun shellView(): View {
        errorView = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            visibility = View.GONE
            setBackgroundColor(Color.WHITE)
            addView(TextView(this@MainActivity).apply {
                text = getString(R.string.hosted_error)
                textSize = 18f
                setTextColor(Color.BLACK)
            })
            addView(Button(this@MainActivity).apply {
                text = getString(R.string.hosted_retry)
                // retry only the last trusted page
                setOnClickListener {
                    errorView.visibility = View.GONE
                    webView.visibility = View.VISIBLE
                    webView.loadUrl(retryUrl)
                }
            })
        }
        webContainer = FrameLayout(this).apply {
            // keep hosted content clear of enforced edge-to-edge system areas
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.VANILLA_ICE_CREAM) {
                val handledInsets = WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout()
                // replace padding on each update while preserving keyboard insets for webview
                setOnApplyWindowInsetsListener { view, insets ->
                    val safeArea = insets.getInsets(handledInsets)
                    view.setPadding(safeArea.left, safeArea.top, safeArea.right, safeArea.bottom)
                    WindowInsets.Builder(insets).setInsets(handledInsets, Insets.NONE).build()
                }
            }
            addView(webView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            addView(errorView, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
        }
        return webContainer
    }

    // create one fully bounded hosted browser
    private fun createWebView(): WebView {
        return WebView(this).also(::configureWebView)
    }

    // apply the production web boundary
    @SuppressLint("SetJavaScriptEnabled")
    private fun configureWebView(view: WebView) {
        // retain native-shell detection and expose this apk's identity before navigation
        view.settings.userAgentString = HostedAppIdentity.userAgent(
            view.settings.userAgentString,
            BuildConfig.VERSION_NAME,
            BuildConfig.VERSION_CODE,
        )
        view.settings.javaScriptEnabled = true
        view.settings.domStorageEnabled = true
        view.settings.allowFileAccess = false
        view.settings.allowContentAccess = false
        view.settings.setSupportMultipleWindows(false)
        view.settings.javaScriptCanOpenWindowsAutomatically = false
        view.settings.mixedContentMode = android.webkit.WebSettings.MIXED_CONTENT_NEVER_ALLOW
        CookieManager.getInstance().setAcceptCookie(true)
        CookieManager.getInstance().setAcceptThirdPartyCookies(view, false)
        view.webChromeClient = object : WebChromeClient() {
            // reject popup windows without a trusted target url
            override fun onCreateWindow(
                view: WebView?,
                isDialog: Boolean,
                isUserGesture: Boolean,
                resultMsg: android.os.Message?,
            ): Boolean = false
        }
        view.webViewClient = object : WebViewClient() {
            // route modern navigation
            override fun shouldOverrideUrlLoading(view: WebView?, request: WebResourceRequest): Boolean {
                return handleNavigation(view, request.url.toString())
            }

            // route legacy navigation
            @Suppress("DEPRECATION")
            override fun shouldOverrideUrlLoading(view: WebView?, url: String): Boolean {
                return handleNavigation(view, url)
            }

            // reset failure state for a new trusted main frame
            override fun onPageStarted(view: WebView?, url: String?, favicon: Bitmap?) {
                // clear only when a trusted navigation actually begins
                if (url != null && HostedNavigationPolicy.isCanonical(url, canonicalOrigin)) {
                    mainFrameFailed = false
                    retryUrl = url
                }
            }

            // retain ordinary certificate validation
            override fun onReceivedSslError(view: WebView?, handler: SslErrorHandler, error: SslError?) {
                handler.cancel()
                showError()
            }

            // show main-frame transport failures only
            override fun onReceivedError(view: WebView?, request: WebResourceRequest, error: WebResourceError) {
                // ignore subresource failures
                if (request.isForMainFrame) {
                    showError()
                }
            }

            // show main-frame http failures only
            override fun onReceivedHttpError(
                view: WebView?,
                request: WebResourceRequest,
                errorResponse: WebResourceResponse,
            ) {
                // ignore subresource failures
                if (request.isForMainFrame && errorResponse.statusCode >= 400) {
                    showError()
                }
            }

            // clear retry state after a trusted page succeeds
            override fun onPageFinished(view: WebView?, url: String?) {
                // reveal only exact-origin pages
                if (url != null && !mainFrameFailed && HostedNavigationPolicy.isCanonical(url, canonicalOrigin)) {
                    errorView.visibility = View.GONE
                    webView.visibility = View.VISIBLE
                }
            }

            // replace an unusable renderer instance
            override fun onRenderProcessGone(view: WebView?, detail: RenderProcessGoneDetail?): Boolean {
                replaceWebView(view ?: webView)
                return true
            }
        }
    }

    // resolve the fixed internal route
    private fun startUrl(intent: Intent?): String {
        val route = intent?.getStringExtra(EXTRA_ROUTE)
        // accept the enum-like forecast route only
        if (route == ROUTE_FORECAST) {
            return "$canonicalOrigin/forecast"
        }
        return canonicalOrigin
    }

    // enforce hosted and external navigation policy
    private fun handleNavigation(view: WebView?, value: String): Boolean {
        // keep exact-origin links in the webview
        if (HostedNavigationPolicy.isCanonical(value, canonicalOrigin)) {
            view?.loadUrl(value)
            return true
        }
        // send the exact privacy mailbox to an email app
        if (HostedNavigationPolicy.isPrivacyContact(value)) {
            try {
                startActivity(Intent(Intent.ACTION_SENDTO, Uri.parse(value)))
            } catch (_: ActivityNotFoundException) {
                showError()
            }
            return true
        }
        // send other safe web links outside the app
        if (HostedNavigationPolicy.isSafeExternal(value, canonicalOrigin)) {
            try {
                startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(value)))
            } catch (_: ActivityNotFoundException) {
                showError()
            }
        }
        return true
    }

    // discard a terminated renderer before offering retry
    private fun replaceWebView(failedView: WebView) {
        webContainer.removeView(failedView)
        failedView.destroy()
        webView = createWebView().apply {
            visibility = View.GONE
        }
        webContainer.addView(
            webView,
            0,
            FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT),
        )
        showError()
    }

    // reveal the local bounded error state
    private fun showError() {
        mainFrameFailed = true
        webView.visibility = View.GONE
        errorView.visibility = View.VISIBLE
    }

    // recompute cached widgets on tap entrypoints
    private fun recomputeWidgets() {
        WidgetController.updateAll(this, Instant.now())
        WidgetWorkScheduler.ensure(this)
    }

    companion object {
        const val EXTRA_ROUTE = "route"
        const val ROUTE_FORECAST = "forecast"
    }
}
