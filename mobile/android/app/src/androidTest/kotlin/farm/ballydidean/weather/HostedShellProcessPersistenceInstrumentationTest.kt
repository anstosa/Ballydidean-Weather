package farm.ballydidean.weather

import android.app.Instrumentation
import android.content.Intent
import android.os.SystemClock
import android.webkit.CookieManager
import android.webkit.WebStorage
import android.webkit.WebView
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import org.json.JSONObject
import org.json.JSONTokener
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test

@Target(AnnotationTarget.CLASS)
@Retention(AnnotationRetention.RUNTIME)
private annotation class ExternalProcessPersistenceTest

@ExternalProcessPersistenceTest
class HostedShellProcessPersistenceInstrumentationTest {
    private val instrumentation: Instrumentation
        get() = InstrumentationRegistry.getInstrumentation()
    private val context
        get() = instrumentation.targetContext
    private val arguments
        get() = InstrumentationRegistry.getArguments()
    private val origin: String
        get() = requiredArgument("fixtureOrigin")

    // execute one externally separated browser-state phase
    @Test
    fun cookieAndLogoutStateSurviveExternalProcessRestart() {
        when (requiredArgument("processPersistencePhase")) {
            "seed" -> seedAuthenticatedState()
            "verify-and-logout" -> verifyAuthenticatedStateAndLogout()
            "verify-logged-out" -> verifyLoggedOutState()
            else -> throw AssertionError("unknown process persistence phase")
        }
    }

    // create real local and httponly browser state
    private fun seedAuthenticatedState() {
        clearBrowserState()
        val activity = launch()
        try {
            val webView = webView(activity)
            awaitHeading(webView, "Fixture home")
            load(webView, "$origin/settings")
            awaitHeading(webView, "Fixture settings")
            click(webView, "Use Celsius")
            awaitBodyMarker(webView, "Public unit preference: Celsius")
            load(webView, "$origin/admin")
            awaitHeading(webView, "Fixture sign in")
            setLabeledValue(webView, "Fixture username", requiredArgument("fixtureUsername"))
            setLabeledValue(webView, "Fixture password", requiredArgument("fixturePassword"))
            clickForNavigation(webView, "Sign in to fixture")
            awaitHeading(webView, "Fixture administration")
            selectLabeledOption(webView, "Fixture server unit", "Celsius")
            clickForNavigation(webView, "Save fixture settings")
            awaitBodyMarker(webView, "Server unit: Celsius")
            CookieManager.getInstance().flush()
            assertFalse(CookieManager.getInstance().getCookie(origin).isNullOrBlank())
        } finally {
            finish(activity)
        }
    }

    // verify process-persistent state then clear the real cookie
    private fun verifyAuthenticatedStateAndLogout() {
        val activity = launch()
        try {
            val webView = webView(activity)
            awaitHeading(webView, "Fixture home")
            load(webView, "$origin/settings")
            awaitBodyMarker(webView, "Public unit preference: Celsius")
            load(webView, "$origin/admin")
            awaitHeading(webView, "Fixture administration")
            awaitBodyMarker(webView, "Authenticated fixture session")
            awaitBodyMarker(webView, "Server unit: Celsius")
            assertFalse(CookieManager.getInstance().getCookie(origin).isNullOrBlank())
            clickForNavigation(webView, "Sign out of fixture")
            awaitBodyMarker(webView, "Fixture session signed out")
            CookieManager.getInstance().flush()
            assertTrue(CookieManager.getInstance().getCookie(origin).isNullOrBlank())
        } finally {
            finish(activity)
        }
    }

    // verify logout remains cleared after another process restart
    private fun verifyLoggedOutState() {
        val activity = launch()
        try {
            val webView = webView(activity)
            awaitHeading(webView, "Fixture home")
            load(webView, "$origin/admin")
            awaitHeading(webView, "Fixture sign in")
            assertTrue(CookieManager.getInstance().getCookie(origin).isNullOrBlank())
        } finally {
            finish(activity)
            clearBrowserState()
        }
    }

    // launch the exact debug fixture origin
    private fun launch(): MainActivity {
        val intent = Intent(context, MainActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            putExtra(HostedTestConfiguration.EXTRA_FIXTURE_ORIGIN, origin)
        }
        return instrumentation.startActivitySync(intent) as MainActivity
    }

    // finish one hosted activity deterministically
    private fun finish(activity: MainActivity) {
        // avoid finishing a destroyed activity
        if (!activity.isFinishing && !activity.isDestroyed) {
            instrumentation.runOnMainSync(activity::finish)
            instrumentation.waitForIdleSync()
        }
    }

    // return the production webview
    private fun webView(activity: MainActivity): WebView {
        return findWebView(activity.window.decorView)
            ?: throw AssertionError("hosted WebView not found")
    }

    // locate the hosted webview recursively
    private fun findWebView(view: android.view.View): WebView? {
        // finish at the browser leaf
        if (view is WebView) {
            return view
        }
        // inspect every container child
        if (view is android.view.ViewGroup) {
            for (index in 0 until view.childCount) {
                val match = findWebView(view.getChildAt(index))
                // return the first browser match
                if (match != null) {
                    return match
                }
            }
        }
        return null
    }

    // navigate the actual webview
    private fun load(webView: WebView, url: String) {
        instrumentation.runOnMainSync { webView.loadUrl(url) }
    }

    // click one frozen fixture label
    private fun click(webView: WebView, label: String) {
        val result = evaluate(
            webView,
            """
            (() => {
              const target = [...document.querySelectorAll('a, button')]
                .find((node) => node.textContent.trim() === ${JSONObject.quote(label)});
              if (!target) return 'missing';
              target.click();
              return 'clicked';
            })()
            """.trimIndent(),
        )
        assertEquals("clicked", result)
    }

    // dispatch one navigation click without awaiting its javascript result
    private fun clickForNavigation(webView: WebView, label: String) {
        val quoted = JSONObject.quote(label)
        await("fixture control $label") {
            evaluate(
                webView,
                "[...document.querySelectorAll('a, button')].some((node) => node.textContent.trim() === $quoted)",
            ) == true
        }
        val script = """
            (() => {
              const target = [...document.querySelectorAll('a, button')]
                .find((node) => node.textContent.trim() === $quoted);
              target?.click();
            })()
        """.trimIndent()
        instrumentation.runOnMainSync { webView.evaluateJavascript(script, null) }
    }

    // fill one frozen labeled input
    private fun setLabeledValue(webView: WebView, label: String, value: String) {
        val result = evaluate(
            webView,
            """
            (() => {
              const label = [...document.querySelectorAll('label')]
                .find((node) => node.textContent.trim() === ${JSONObject.quote(label)});
              const control = label && document.getElementById(label.htmlFor);
              if (!control) return 'missing';
              control.value = ${JSONObject.quote(value)};
              control.dispatchEvent(new Event('input', { bubbles: true }));
              return 'set';
            })()
            """.trimIndent(),
        )
        assertEquals("set", result)
    }

    // choose one frozen select option
    private fun selectLabeledOption(webView: WebView, label: String, option: String) {
        val result = evaluate(
            webView,
            """
            (() => {
              const label = [...document.querySelectorAll('label')]
                .find((node) => node.textContent.trim() === ${JSONObject.quote(label)});
              const control = label && document.getElementById(label.htmlFor);
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
        await("heading $expected") {
            evaluate(webView, "document.querySelector('h1')?.textContent.trim() ?? null") == expected
        }
    }

    // wait for one body marker
    private fun awaitBodyMarker(webView: WebView, expected: String) {
        await("body marker $expected") {
            evaluate(webView, "document.body?.innerText.includes(${JSONObject.quote(expected)}) === true") == true
        }
    }

    // evaluate one bounded fixture expression
    private fun evaluate(webView: WebView, script: String): Any? {
        val latch = CountDownLatch(1)
        var encoded: String? = null
        instrumentation.runOnMainSync {
            webView.evaluateJavascript(script) {
                encoded = it
                latch.countDown()
            }
        }
        // let polling callers retry when no callback arrives within this bound
        if (!latch.await(500, TimeUnit.MILLISECONDS)) {
            return null
        }
        return encoded?.let { JSONTokener(it).nextValue() }
    }

    // poll one asynchronous browser condition
    private fun await(label: String, condition: () -> Boolean) {
        repeat(50) {
            instrumentation.waitForIdleSync()
            // finish once the condition holds
            if (condition()) {
                return
            }
            SystemClock.sleep(100)
        }
        throw AssertionError("timed out waiting for $label")
    }

    // clear only fixture-owned browser state
    private fun clearBrowserState() {
        val latch = CountDownLatch(1)
        instrumentation.runOnMainSync {
            CookieManager.getInstance().removeAllCookies { latch.countDown() }
            WebStorage.getInstance().deleteAllData()
        }
        assertTrue(latch.await(5, TimeUnit.SECONDS))
        CookieManager.getInstance().flush()
    }

    // require one fixture runner argument
    private fun requiredArgument(name: String): String {
        val value = arguments.getString(name)
        assertNotNull("missing $name", value)
        return checkNotNull(value)
    }
}
