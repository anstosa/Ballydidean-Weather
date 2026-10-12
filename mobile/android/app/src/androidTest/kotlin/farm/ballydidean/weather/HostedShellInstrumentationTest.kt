package farm.ballydidean.weather

import android.app.Activity
import android.app.Instrumentation
import android.content.Intent
import android.os.Build
import android.os.SystemClock
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.WebStorage
import android.webkit.WebView
import android.widget.Button
import android.widget.TextView
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import farm.ballydidean.weather.widget.WeatherWidgetRenderer
import farm.ballydidean.weather.widget.WidgetStorage
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import org.json.JSONObject
import org.json.JSONTokener
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

class HostedShellInstrumentationTest {
    private val instrumentation: Instrumentation
        get() = InstrumentationRegistry.getInstrumentation()
    private val context
        get() = instrumentation.targetContext
    private val arguments
        get() = InstrumentationRegistry.getArguments()
    private val origin: String
        get() = requiredArgument("fixtureOrigin")
    private val untrustedOrigin: String
        get() = requiredArgument("untrustedFixtureOrigin")

    // clear persistent browser state before each journey
    @Before
    fun clearBrowserStateBefore() {
        clearBrowserState()
    }

    // clear persistent browser state after each journey
    @After
    fun clearBrowserStateAfter() {
        clearBrowserState()
    }

    // exercise hosted routes back navigation and blocked targets
    @Test
    fun hostedRoutesBackPopupAndPolicyStayBounded() {
        val activity = launch()
        try {
            val webView = webView(activity)
            awaitHeading(webView, "Fixture home")
            awaitPageComplete(webView)
            // verify the running apk exposes its own build without losing hosted-shell detection
            val userAgent = evaluate(webView, "navigator.userAgent") as String
            assertTrue(userAgent.contains("; wv)"))
            assertTrue(userAgent.contains(" BallydideanWeather/${BuildConfig.VERSION_NAME} "))
            assertTrue(userAgent.endsWith("BallydideanWeatherBuild/${BuildConfig.VERSION_CODE}"))
            click(webView, "Open fixture forecast")
            awaitHeading(webView, "Fixture forecast")
            awaitPageComplete(webView)
            await("committed fixture history") { canGoBack(webView) }
            instrumentation.runOnMainSync(activity::onBackPressed)
            awaitHeading(webView, "Fixture home")
            click(webView, "Open fixture map in new window")
            SystemClock.sleep(500)
            assertEquals("Fixture home", heading(webView))
            // exercise every retained hosted route
            for (route in listOf("map", "logs", "trends", "settings")) {
                load(webView, "$origin/$route")
                awaitHeading(webView, "Fixture $route")
            }
            load(webView, "$origin/policy")
            awaitHeading(webView, "Fixture policy")
            val privacyMonitor = ExternalIntentMonitor(Intent.ACTION_SENDTO)
            instrumentation.addMonitor(privacyMonitor)
            try {
                clickInjectedLink(webView, "mailto:sanctuary@ballydidean.farm")
                awaitExternalIntent(
                    privacyMonitor,
                    Intent.ACTION_SENDTO,
                    "mailto:sanctuary@ballydidean.farm",
                )
                assertEquals("Fixture policy", heading(webView))
            } finally {
                instrumentation.removeMonitor(privacyMonitor)
            }
            val externalMonitor = ExternalIntentMonitor(Intent.ACTION_VIEW)
            instrumentation.addMonitor(externalMonitor)
            try {
                click(webView, "Unsafe HTTP fixture")
                SystemClock.sleep(300)
                assertEquals(null, externalMonitor.intent.get())
                assertEquals("Fixture policy", heading(webView))
                click(webView, "Lookalike Weather origin")
                SystemClock.sleep(300)
                assertEquals(null, externalMonitor.intent.get())
                assertEquals("Fixture policy", heading(webView))
                tap(webView, "External fixture policy")
                awaitExternalIntent(
                    externalMonitor,
                    Intent.ACTION_VIEW,
                    "https://external.example.invalid/fixture",
                )
                assertEquals("Fixture policy", heading(webView))
            } finally {
                instrumentation.removeMonitor(externalMonitor)
            }
        } finally {
            finish(activity)
        }
    }

    // preserve local and httponly session state across recreation
    @Test
    fun loginSettingsPersistenceAndLogoutUseTheRealWebView() {
        var activity = launch()
        try {
            var webView = webView(activity)
            awaitHeading(webView, "Fixture home")
            load(webView, "$origin/settings")
            awaitHeading(webView, "Fixture settings")
            click(webView, "Use Celsius")
            awaitBodyMarker(webView, "Public unit preference: Celsius")
            load(webView, "$origin/admin")
            awaitHeading(webView, "Fixture sign in")
            setLabeledValue(webView, "Fixture username", requiredArgument("fixtureUsername"))
            setLabeledValue(webView, "Fixture password", requiredArgument("fixturePassword"))
            click(webView, "Sign in to fixture")
            awaitHeading(webView, "Fixture administration")
            awaitBodyMarker(webView, "Authenticated fixture session")
            awaitBodyMarker(webView, "HttpOnly session hidden")
            CookieManager.getInstance().flush()
            assertFalse(CookieManager.getInstance().getCookie(origin).isNullOrBlank())
            selectLabeledOption(webView, "Fixture server unit", "Celsius")
            click(webView, "Save fixture settings")
            awaitBodyMarker(webView, "Server unit: Celsius")
            val cachedBytes = WidgetStorage(context).readSnapshotBytes()?.toString(Charsets.UTF_8).orEmpty()
            assertFalse(cachedBytes.contains(requiredArgument("fixtureUsername")))
            finish(activity)

            activity = launch()
            webView = webView(activity)
            awaitHeading(webView, "Fixture home")
            load(webView, "$origin/settings")
            awaitBodyMarker(webView, "Public unit preference: Celsius")
            load(webView, "$origin/admin")
            awaitHeading(webView, "Fixture administration")
            awaitBodyMarker(webView, "Server unit: Celsius")
            click(webView, "Sign out of fixture")
            awaitBodyMarker(webView, "Fixture session signed out")
            CookieManager.getInstance().flush()
            assertTrue(CookieManager.getInstance().getCookie(origin).isNullOrBlank())
            load(webView, "$origin/admin")
            awaitHeading(webView, "Fixture sign in")
        } finally {
            finish(activity)
        }
    }

    // cancel an untrusted certificate and recover through retry
    @Test
    fun certificateFailureNeverReceivesAWebViewTrustBypass() {
        val activity = launch()
        try {
            val webView = webView(activity)
            awaitHeading(webView, "Fixture home")
            load(webView, "$untrustedOrigin/tls-negative")
            awaitVisibleText(activity, "Weather is unavailable")
            SystemClock.sleep(500)
            awaitVisibleText(activity, "Weather is unavailable")
            val retry = findViews(activity.window.decorView)
                .filterIsInstance<Button>()
                .single { it.text.toString() == "Retry" }
            instrumentation.runOnMainSync(retry::performClick)
            awaitHeading(webView, "Fixture home")
        } finally {
            finish(activity)
        }
    }

    // replace a terminated renderer before retrying trusted content
    @Test
    fun rendererTerminationCreatesAFreshWebViewForRetry() {
        val activity = launch()
        try {
            val failedWebView = webView(activity)
            awaitHeading(failedWebView, "Fixture home")
            load(failedWebView, "chrome://crash")
            awaitVisibleText(activity, "Weather is unavailable")
            val replacementWebView = awaitReplacementWebView(activity, failedWebView)
            val retry = findViews(activity.window.decorView)
                .filterIsInstance<Button>()
                .single { it.text.toString() == "Retry" }
            instrumentation.runOnMainSync(retry::performClick)
            awaitHeading(replacementWebView, "Fixture home")
        } finally {
            finish(activity)
        }
    }

    // reject untrusted routes and preserve immutable widget routing
    @Test
    fun immutableWidgetIntentOpensOnlyTheForecastRoute() {
        val activity = launch(route = "https://lookalike.invalid/settings")
        var destination: MainActivity? = null
        try {
            val webView = webView(activity)
            awaitHeading(webView, "Fixture home")
            val pendingIntent = WeatherWidgetRenderer.forecastPendingIntent(context, 98_765)
            // require platform immutable identity where exposed
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                assertTrue(pendingIntent.isImmutable)
            }
            assertEquals(context.packageName, pendingIntent.creatorPackage)
            pendingIntent.send()
            destination = awaitReplacementActivity(activity)
            val destinationWebView = webView(checkNotNull(destination))
            awaitHeading(destinationWebView, "Fixture forecast")
            assertEquals("$origin/forecast", currentUrl(destinationWebView))
        } finally {
            // close the destination before the replaced source activity
            destination?.let(::finish)
            finish(activity)
        }
    }

    // launch only the fixed debug https origin
    private fun launch(route: String? = null): MainActivity {
        val intent = Intent(context, MainActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            putExtra(HostedTestConfiguration.EXTRA_FIXTURE_ORIGIN, origin)
            // include only an explicit route under test
            if (route != null) {
                putExtra(MainActivity.EXTRA_ROUTE, route)
            }
        }
        return instrumentation.startActivitySync(intent) as MainActivity
    }

    // finish one hosted activity deterministically
    private fun finish(activity: MainActivity) {
        // avoid a second finish after an intentional recreation
        if (!activity.isFinishing && !activity.isDestroyed) {
            instrumentation.runOnMainSync(activity::finish)
            instrumentation.waitForIdleSync()
        }
    }

    // find the production webview inside the hosted shell
    private fun webView(activity: MainActivity): WebView {
        return findViews(activity.window.decorView).filterIsInstance<WebView>().single()
    }

    // flatten one live view tree
    private fun findViews(root: View): List<View> {
        val result = mutableListOf<View>()
        fun visit(view: View) {
            result += view
            // visit every hosted child
            if (view is ViewGroup) {
                for (index in 0 until view.childCount) {
                    visit(view.getChildAt(index))
                }
            }
        }
        visit(root)
        return result
    }

    // navigate the actual webview
    private fun load(webView: WebView, url: String) {
        instrumentation.runOnMainSync { webView.loadUrl(url) }
    }

    // click one frozen fixture label
    private fun click(webView: WebView, label: String) {
        val quoted = JSONObject.quote(label)
        val result = evaluate(
            webView,
            """
            (() => {
              const target = [...document.querySelectorAll('a, button')]
                .find((node) => node.textContent.trim() === $quoted);
              if (!target) return 'missing';
              target.click();
              return 'clicked';
            })()
            """.trimIndent(),
        )
        assertEquals("clicked", result)
    }

    // click one exact injected policy link
    private fun clickInjectedLink(webView: WebView, href: String) {
        val result = evaluate(
            webView,
            """
            (() => {
              const target = document.createElement('a');
              target.href = ${JSONObject.quote(href)};
              target.textContent = 'Injected policy link';
              document.body.append(target);
              target.click();
              return 'clicked';
            })()
            """.trimIndent(),
        )
        assertEquals("clicked", result)
    }

    // tap one fixture label through the input dispatcher
    private fun tap(webView: WebView, label: String) {
        val bounds = evaluate(
            webView,
            """
            (() => {
              const target = [...document.querySelectorAll('a, button')]
                .find((node) => node.textContent.trim() === ${JSONObject.quote(label)});
              if (!target) return null;
              target.scrollIntoView({ block: 'center', inline: 'center' });
              const rect = target.getBoundingClientRect();
              return {
                x: rect.left + (rect.width / 2),
                y: rect.top + (rect.height / 2),
                viewportWidth: window.innerWidth,
                viewportHeight: window.innerHeight,
              };
            })()
            """.trimIndent(),
        ) as? JSONObject
        assertNotNull("missing fixture control $label", bounds)
        val location = IntArray(2)
        var width = 0
        var height = 0
        instrumentation.runOnMainSync {
            webView.getLocationOnScreen(location)
            width = webView.width
            height = webView.height
        }
        val target = checkNotNull(bounds)
        val x = location[0] + target.getDouble("x") * width / target.getDouble("viewportWidth")
        val y = location[1] + target.getDouble("y") * height / target.getDouble("viewportHeight")
        val downAt = SystemClock.uptimeMillis()
        instrumentation.sendPointerSync(MotionEvent.obtain(downAt, downAt, MotionEvent.ACTION_DOWN, x.toFloat(), y.toFloat(), 0))
        instrumentation.sendPointerSync(MotionEvent.obtain(downAt, SystemClock.uptimeMillis(), MotionEvent.ACTION_UP, x.toFloat(), y.toFloat(), 0))
    }

    // fill one control selected by its frozen label
    private fun setLabeledValue(webView: WebView, label: String, value: String) {
        val result = evaluate(
            webView,
            """
            (() => {
              const label = [...document.querySelectorAll('label')]
                .find((node) => node.textContent.trim() === ${JSONObject.quote(label)});
              const control = label && (document.getElementById(label.htmlFor) || label.querySelector('input'));
              if (!control) return 'missing';
              control.value = ${JSONObject.quote(value)};
              control.dispatchEvent(new Event('input', { bubbles: true }));
              return 'set';
            })()
            """.trimIndent(),
        )
        assertEquals("set", result)
    }

    // select one server preference by its frozen label
    private fun selectLabeledOption(webView: WebView, label: String, option: String) {
        val result = evaluate(
            webView,
            """
            (() => {
              const label = [...document.querySelectorAll('label')]
                .find((node) => node.textContent.trim() === ${JSONObject.quote(label)});
              const control = label && (document.getElementById(label.htmlFor) || label.querySelector('select'));
              const selected = control && [...control.options]
                .find((item) => item.textContent.trim() === ${JSONObject.quote(option)});
              if (!selected) return 'missing';
              control.value = selected.value;
              control.dispatchEvent(new Event('change', { bubbles: true }));
              return 'selected';
            })()
            """.trimIndent(),
        )
        assertEquals("selected", result)
    }

    // wait for one page heading
    private fun awaitHeading(webView: WebView, expected: String) {
        await("heading $expected") { heading(webView) == expected }
    }

    // wait for native and document navigation completion
    private fun awaitPageComplete(webView: WebView) {
        await("completed page") {
            var progress = 0
            instrumentation.runOnMainSync { progress = webView.progress }
            progress == 100 && evaluate(webView, "document.readyState") == "complete"
        }
    }

    // read one page heading
    private fun heading(webView: WebView): String? {
        return evaluate(webView, "document.querySelector('h1')?.textContent.trim() ?? null") as? String
    }

    // wait for one body marker
    private fun awaitBodyMarker(webView: WebView, expected: String) {
        await("body marker $expected") {
            evaluate(webView, "document.body?.innerText.includes(${JSONObject.quote(expected)}) === true") == true
        }
    }

    // wait for one native error label
    private fun awaitVisibleText(activity: MainActivity, expected: String) {
        await("visible text $expected") {
            findViews(activity.window.decorView).filterIsInstance<TextView>()
                .any { it.visibility == View.VISIBLE && it.text.toString() == expected }
        }
    }

    // verify exact external intent routing
    private fun awaitExternalIntent(monitor: ExternalIntentMonitor, expectedAction: String, expectedUri: String) {
        await("external intent $expectedUri") { monitor.intent.get() != null }
        val intent = checkNotNull(monitor.intent.get())
        assertEquals(expectedAction, intent.action)
        assertEquals(expectedUri, intent.dataString)
        assertEquals(1, monitor.hits)
    }

    // wait for the dead browser to leave the view tree
    private fun awaitReplacementWebView(activity: MainActivity, previous: WebView): WebView {
        var replacement: WebView? = null
        await("replacement WebView") {
            replacement = findViews(activity.window.decorView)
                .filterIsInstance<WebView>()
                .singleOrNull()
                ?.takeUnless { it === previous }
            replacement != null
        }
        return checkNotNull(replacement)
    }

    // read one current url on the ui thread
    private fun currentUrl(webView: WebView): String? {
        var value: String? = null
        instrumentation.runOnMainSync { value = webView.url }
        return value
    }

    // read web history on the ui thread
    private fun canGoBack(webView: WebView): Boolean {
        var value = false
        instrumentation.runOnMainSync { value = webView.canGoBack() }
        return value
    }

    // wait for the immutable pending intent destination activity
    private fun awaitReplacementActivity(previous: MainActivity): MainActivity {
        var replacement: MainActivity? = null
        await("widget forecast activity") {
            replacement = resumedMainActivity()?.takeUnless { it === previous }
            replacement != null
        }
        return checkNotNull(replacement)
    }

    // find the one currently resumed hosted activity
    private fun resumedMainActivity(): MainActivity? {
        var value: MainActivity? = null
        instrumentation.runOnMainSync {
            value = ActivityLifecycleMonitorRegistry.getInstance()
                .getActivitiesInStage(Stage.RESUMED)
                .filterIsInstance<MainActivity>()
                .singleOrNull()
        }
        return value
    }

    // evaluate one deterministic fixture expression
    private fun evaluate(webView: WebView, script: String): Any? {
        val latch = CountDownLatch(1)
        var encoded: String? = null
        instrumentation.runOnMainSync {
            webView.evaluateJavascript(script) {
                encoded = it
                latch.countDown()
            }
        }
        // let polling callers retry across navigation callback cancellation
        if (!latch.await(500, TimeUnit.MILLISECONDS)) {
            return null
        }
        return encoded?.let { JSONTokener(it).nextValue() }
    }

    // poll one asynchronous web condition
    private fun await(label: String, condition: () -> Boolean) {
        repeat(50) {
            instrumentation.waitForIdleSync()
            // finish once the observed condition holds
            if (condition()) {
                return
            }
            SystemClock.sleep(100)
        }
        throw AssertionError("timed out waiting for $label")
    }

    // require one fixture runner argument
    private fun requiredArgument(name: String): String {
        val value = arguments.getString(name)
        assertNotNull("run through run-hosted-shell-tests.sh; missing $name", value)
        return checkNotNull(value)
    }

    // remove only fixture-owned browser data
    private fun clearBrowserState() {
        val latch = CountDownLatch(1)
        instrumentation.runOnMainSync {
            CookieManager.getInstance().removeAllCookies { latch.countDown() }
            WebStorage.getInstance().deleteAllData()
        }
        assertTrue(latch.await(5, TimeUnit.SECONDS))
        CookieManager.getInstance().flush()
    }

    // capture only external browser launches
    private class ExternalIntentMonitor(private val expectedAction: String) : Instrumentation.ActivityMonitor() {
        val intent = AtomicReference<Intent?>()

        // block the exact external launch under test
        override fun onStartActivity(candidate: Intent): Instrumentation.ActivityResult? {
            // ignore unrelated activity launches
            if (candidate.action != expectedAction) {
                return null
            }
            intent.set(Intent(candidate))
            return Instrumentation.ActivityResult(Activity.RESULT_OK, null)
        }
    }
}
