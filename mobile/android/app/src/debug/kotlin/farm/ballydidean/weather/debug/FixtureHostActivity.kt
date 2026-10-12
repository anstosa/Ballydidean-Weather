package farm.ballydidean.weather.debug

import android.app.Activity
import android.appwidget.AppWidgetHost
import android.appwidget.AppWidgetHostView
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProviderInfo
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.FrameLayout
import android.widget.RemoteViews
import android.widget.TextView
import farm.ballydidean.weather.R
import farm.ballydidean.weather.widget.WeatherWidgetProvider

class FixtureHostActivity : Activity() {
    private lateinit var widgetHost: AppWidgetHost
    private lateinit var activeOptions: Bundle
    private lateinit var activeVariant: FixtureVariant
    private var allocatedWidgetId: Int? = null
    private var activeLandscape = false
    private var acceptsHostUpdates = true
    var renderedHostView: AppWidgetHostView? = null
        private set
    var fixtureRestorationCount = 0
        private set

    // bind the real provider into a test-only host
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val (widthDp, heightDp) = requestedBounds()
        val variant = FixtureVariant.fromWireName(intent.getStringExtra(EXTRA_VARIANT))
        activeOptions = widgetOptions(widthDp, heightDp)
        activeVariant = variant
        activeLandscape = heightDp == LANDSCAPE_HEIGHT_DP
        widgetHost = FixtureAppWidgetHost(this, HOST_ID, ::restoreFixtureAfterProviderUpdate)
        widgetHost.startListening()
        val root = FrameLayout(this).apply {
            setBackgroundColor(Color.rgb(238, 238, 238))
        }
        setContentView(root)
        bindWidget(root, widthDp, heightDp, variant)
    }

    // release the short-lived host allocation
    override fun onDestroy() {
        acceptsHostUpdates = false
        widgetHost.stopListening()
        allocatedWidgetId?.let(widgetHost::deleteAppWidgetId)
        super.onDestroy()
    }

    // resize one bound host instance through real options
    fun resizeWidget(widthDp: Int, heightDp: Int, variant: FixtureVariant) {
        val appWidgetId = checkNotNull(allocatedWidgetId)
        val hostView = checkNotNull(renderedHostView)
        val options = widgetOptions(widthDp, heightDp)
        activeOptions = options
        activeVariant = variant
        activeLandscape = heightDp == LANDSCAPE_HEIGHT_DP
        val bounds = hostView.outerBounds(widthDp, heightDp)
        hostView.layoutParams = (hostView.layoutParams as FrameLayout.LayoutParams).apply {
            width = bounds.widthPx
            height = bounds.heightPx
        }
        // pass the full host area so the platform subtracts its own padding once
        hostView.updateAppWidgetSize(
            options,
            bounds.widthDp,
            bounds.heightDp,
            bounds.widthDp,
            bounds.heightDp,
        )
        applyFixture(appWidgetId, options, variant)
    }

    // replay the real provider broadcast for lifecycle regression coverage
    fun requestProviderUpdate() {
        val appWidgetId = checkNotNull(allocatedWidgetId)
        val provider = ComponentName(this, WeatherWidgetProvider::class.java)
        sendBroadcast(
            Intent(AppWidgetManager.ACTION_APPWIDGET_UPDATE)
                .setComponent(provider)
                .putExtra(AppWidgetManager.EXTRA_APPWIDGET_IDS, intArrayOf(appWidgetId)),
        )
    }

    // permit only approved widget content dimensions
    private fun requestedBounds(): Pair<Int, Int> {
        val requestedWidth = intent.getIntExtra(EXTRA_WIDTH_DP, PORTRAIT_WIDTH_DP)
        val requestedHeight = intent.getIntExtra(EXTRA_HEIGHT_DP, PORTRAIT_HEIGHT_DP)
        // select the official typical landscape minimum
        if (requestedWidth == LANDSCAPE_WIDTH_DP && requestedHeight == LANDSCAPE_HEIGHT_DP) {
            return LANDSCAPE_WIDTH_DP to LANDSCAPE_HEIGHT_DP
        }
        return PORTRAIT_WIDTH_DP to PORTRAIT_HEIGHT_DP
    }

    // perform the platform host binding flow
    private fun bindWidget(root: FrameLayout, widthDp: Int, heightDp: Int, variant: FixtureVariant) {
        val manager = AppWidgetManager.getInstance(this)
        val provider = ComponentName(this, WeatherWidgetProvider::class.java)
        val appWidgetId = widgetHost.allocateAppWidgetId()
        allocatedWidgetId = appWidgetId
        // surface missing shell-granted bind authority
        if (!manager.bindAppWidgetIdIfAllowed(appWidgetId, provider, activeOptions)) {
            root.addView(TextView(this).apply {
                text = "fixture host requires: cmd appwidget grantbind --package $packageName"
                setTextColor(Color.BLACK)
                textSize = 16f
                gravity = Gravity.CENTER
            }, FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT))
            return
        }
        val providerInfo = manager.getAppWidgetInfo(appWidgetId)
        val hostView = widgetHost.createView(this, appWidgetId, providerInfo)
        renderedHostView = hostView
        val bounds = hostView.outerBounds(widthDp, heightDp)
        val layoutParams = FrameLayout.LayoutParams(bounds.widthPx, bounds.heightPx).apply {
            gravity = Gravity.CENTER
        }
        root.addView(hostView, layoutParams)
        applyFixture(appWidgetId, activeOptions, variant)
    }

    // restore the selected fixture after any late provider remoteviews
    private fun restoreFixtureAfterProviderUpdate(hostView: AppWidgetHostView) {
        // ignore teardown and already-correct fixture deliveries
        if (!acceptsHostUpdates || matchesFixture(hostView, activeVariant, activeLandscape)) {
            return
        }
        val appWidgetId = allocatedWidgetId ?: return
        fixtureRestorationCount += 1
        applyFixture(appWidgetId, activeOptions, activeVariant)
    }

    // apply deterministic content through the production renderer
    private fun applyFixture(appWidgetId: Int, options: Bundle, variant: FixtureVariant) {
        WeatherWidgetProvider.updateWidget(
            this,
            AppWidgetManager.getInstance(this),
            appWidgetId,
            options,
            DebugWidgetFixtures.fixture(variant).presentation,
        )
    }

    // build exact appwidget option bounds
    private fun widgetOptions(widthDp: Int, heightDp: Int): Bundle {
        return Bundle().apply {
            putInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, widthDp)
            putInt(AppWidgetManager.OPTION_APPWIDGET_MAX_WIDTH, widthDp)
            putInt(AppWidgetManager.OPTION_APPWIDGET_MIN_HEIGHT, heightDp)
            putInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT, heightDp)
            putInt(AppWidgetManager.OPTION_APPWIDGET_HOST_CATEGORY, AppWidgetProviderInfo.WIDGET_CATEGORY_HOME_SCREEN)
        }
    }

    companion object {
        const val EXTRA_WIDTH_DP = "widthDp"
        const val EXTRA_HEIGHT_DP = "heightDp"
        const val EXTRA_VARIANT = "variant"
        const val PORTRAIT_WIDTH_DP = 276
        const val PORTRAIT_HEIGHT_DP = 102
        const val LANDSCAPE_WIDTH_DP = 554
        const val LANDSCAPE_HEIGHT_DP = 51
        private const val HOST_ID = 0x57454154

        // identify the selected fixture after production remoteviews inflation
        internal fun matchesFixture(
            view: View,
            variant: FixtureVariant,
            @Suppress("UNUSED_PARAMETER") landscape: Boolean,
        ): Boolean {
            val presentation = DebugWidgetFixtures.fixture(variant).presentation
            // collect the visible weather segments
            val slotRoots = buildList { collectVisibleViews(view, R.id.slot_root, this) }
            // identify every requested weather label
            val expectedLabels = presentation.groups.map { it.hourLabel to it.temperatureLabel }
            // observe every rendered weather label
            val renderedLabels = slotRoots.map { slot ->
                slot.findViewById<TextView>(R.id.slot_hour)?.text?.toString() to
                    slot.findViewById<TextView>(R.id.slot_temperature)?.text?.toString()
            }
            // identify every requested segment description
            val expectedDescriptions = presentation.groups.map { it.accessibilityLabel() }
            // observe every rendered segment description
            val renderedDescriptions = slotRoots.map { it.contentDescription?.toString() }
            // require the complete ordered weather signature
            if (renderedLabels != expectedLabels || renderedDescriptions != expectedDescriptions) {
                return false
            }
            // collect the visible remainder segments
            val messageRoots = buildList { collectVisibleViews(view, R.id.bedtime_root, this) }
            // reject any message segment from weather-only fixtures
            if (presentation.message == null) {
                return messageRoots.isEmpty()
            }
            // require one complete remainder message
            if (messageRoots.size != 1) {
                return false
            }
            val messageRoot = messageRoots.single()
            val renderedMessage = messageRoot.findViewById<TextView>(R.id.bedtime_message)?.text?.toString()
            val renderedHour = messageRoot.findViewById<TextView>(R.id.bedtime_hour)?.text?.toString()
            // distinguish bedtime from unavailable messages
            val expectedHour = if (presentation.showBedtime) "Overnight" else ""
            // match the renderer's accessible remainder summary
            val expectedDescription = presentation.overnight?.accessibilityLabel()
                ?: if (presentation.showBedtime) {
                    "Overnight, 8 PM through 7 AM, forecast unavailable"
                } else {
                    presentation.message
                }
            return renderedMessage == presentation.message &&
                renderedHour == expectedHour &&
                messageRoot.contentDescription?.toString() == expectedDescription
        }

        // collect visible semantic roots in hierarchy order
        private fun collectVisibleViews(view: View, targetId: Int, destination: MutableList<View>) {
            // retain only visible matching roots
            if (view.id == targetId && view.visibility == View.VISIBLE) {
                destination += view
            }
            // visit every inflated remoteviews child
            if (view is ViewGroup) {
                // traverse children in presentation order
                for (index in 0 until view.childCount) {
                    collectVisibleViews(view.getChildAt(index), targetId, destination)
                }
            }
        }
    }
}

// hold outer host dimensions in both coordinate systems
private data class FixtureHostBounds(
    val widthPx: Int,
    val heightPx: Int,
    val widthDp: Int,
    val heightDp: Int,
)

// include platform host padding around the requested remoteviews content
private fun AppWidgetHostView.outerBounds(contentWidthDp: Int, contentHeightDp: Int): FixtureHostBounds {
    val density = resources.displayMetrics.density
    val horizontalPadding = paddingLeft + paddingRight
    val verticalPadding = paddingTop + paddingBottom
    return FixtureHostBounds(
        widthPx = (contentWidthDp * density).toInt() + horizontalPadding,
        heightPx = (contentHeightDp * density).toInt() + verticalPadding,
        widthDp = contentWidthDp + (horizontalPadding / density).toInt(),
        heightDp = contentHeightDp + (verticalPadding / density).toInt(),
    )
}

private class FixtureAppWidgetHost(
    context: Context,
    hostId: Int,
    private val onRemoteViewsUpdated: (AppWidgetHostView) -> Unit,
) : AppWidgetHost(context, hostId) {
    // create an observing real host view
    override fun onCreateView(
        context: Context,
        appWidgetId: Int,
        appWidget: AppWidgetProviderInfo,
    ): AppWidgetHostView {
        return FixtureAppWidgetHostView(context, onRemoteViewsUpdated)
    }
}

private class FixtureAppWidgetHostView(
    context: Context,
    private val onRemoteViewsUpdated: (AppWidgetHostView) -> Unit,
) : AppWidgetHostView(context) {
    // report each fully applied provider update
    override fun updateAppWidget(remoteViews: RemoteViews?) {
        super.updateAppWidget(remoteViews)
        onRemoteViewsUpdated(this)
    }
}
