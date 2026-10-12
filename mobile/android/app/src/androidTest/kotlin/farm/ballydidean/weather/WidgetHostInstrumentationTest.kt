package farm.ballydidean.weather

import android.app.Instrumentation
import android.appwidget.AppWidgetManager
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Path
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.BitmapDrawable
import android.graphics.drawable.GradientDrawable
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.os.Bundle
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.View.MeasureSpec
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView
import androidx.test.platform.app.InstrumentationRegistry
import androidx.work.NetworkType
import androidx.work.WorkManager
import farm.ballydidean.weather.debug.DebugWidgetFixtures
import farm.ballydidean.weather.debug.FixtureHostActivity
import farm.ballydidean.weather.debug.FixtureVariant
import farm.ballydidean.weather.widget.WidgetGroup
import farm.ballydidean.weather.widget.WidgetTemperatureTone
import farm.ballydidean.weather.widget.WeatherWidgetRenderer
import java.io.File
import java.io.FileOutputStream
import java.time.Duration
import java.time.Instant
import java.util.concurrent.TimeUnit
import kotlin.math.abs
import kotlin.math.roundToInt
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class WidgetHostInstrumentationTest {
    private val instrumentation: Instrumentation
        get() = InstrumentationRegistry.getInstrumentation()

    // verify portrait maximum content on a real host view
    @Test
    fun testMaximumPortrait() {
        verifyRenderedWidget(
            FixtureHostActivity.PORTRAIT_WIDTH_DP,
            FixtureHostActivity.PORTRAIT_HEIGHT_DP,
            FixtureVariant.MAXIMUM,
            expectedGroups = 5,
            expectedBedtime = false,
        )
    }

    // verify landscape maximum content on a real host view
    @Test
    fun testMaximumLandscape() {
        verifyRenderedWidget(
            FixtureHostActivity.LANDSCAPE_WIDTH_DP,
            FixtureHostActivity.LANDSCAPE_HEIGHT_DP,
            FixtureVariant.MAXIMUM,
            expectedGroups = 5,
            expectedBedtime = false,
        )
    }

    // verify near-cutoff weather and bedtime together
    @Test
    fun testNearCutoffPortrait() {
        // prove overnight first appears beside four weather fifths at 4 PM
        verifyRenderedWidget(
            FixtureHostActivity.PORTRAIT_WIDTH_DP,
            FixtureHostActivity.PORTRAIT_HEIGHT_DP,
            FixtureVariant.OVERNIGHT_FIFTH,
            expectedGroups = 4,
            expectedBedtime = true,
        )
        verifyRenderedWidget(
            FixtureHostActivity.PORTRAIT_WIDTH_DP,
            FixtureHostActivity.PORTRAIT_HEIGHT_DP,
            FixtureVariant.NEAR_CUTOFF,
            expectedGroups = 1,
            expectedBedtime = true,
        )
    }

    // verify all post-cutoff space stays non-weather
    @Test
    fun testAllBedtimeLandscape() {
        // retain the first overnight row in the compact host
        verifyRenderedWidget(
            FixtureHostActivity.LANDSCAPE_WIDTH_DP,
            FixtureHostActivity.LANDSCAPE_HEIGHT_DP,
            FixtureVariant.OVERNIGHT_FIFTH,
            expectedGroups = 4,
            expectedBedtime = true,
        )
        // prove the compact 7 PM row grants four fifths to overnight
        verifyRenderedWidget(
            FixtureHostActivity.LANDSCAPE_WIDTH_DP,
            FixtureHostActivity.LANDSCAPE_HEIGHT_DP,
            FixtureVariant.NEAR_CUTOFF,
            expectedGroups = 1,
            expectedBedtime = true,
        )
        verifyRenderedWidget(
            FixtureHostActivity.LANDSCAPE_WIDTH_DP,
            FixtureHostActivity.LANDSCAPE_HEIGHT_DP,
            FixtureVariant.ALL_BEDTIME,
            expectedGroups = 0,
            expectedBedtime = true,
        )
    }

    // verify stale provenance keeps full weather and accessible credit
    @Test
    fun testStalePortrait() {
        verifyRenderedWidget(
            FixtureHostActivity.PORTRAIT_WIDTH_DP,
            FixtureHostActivity.PORTRAIT_HEIGHT_DP,
            FixtureVariant.STALE,
            expectedGroups = 5,
            expectedBedtime = false,
        )
    }

    // verify mixed raw provenance stays visible
    @Test
    fun testRawMixedPortrait() {
        verifyRenderedWidget(
            FixtureHostActivity.PORTRAIT_WIDTH_DP,
            FixtureHostActivity.PORTRAIT_HEIGHT_DP,
            FixtureVariant.RAW_MIXED,
            expectedGroups = 5,
            expectedBedtime = false,
        )
    }

    // verify pure raw provenance stays visible
    @Test
    fun testRawPortrait() {
        verifyRenderedWidget(
            FixtureHostActivity.PORTRAIT_WIDTH_DP,
            FixtureHostActivity.PORTRAIT_HEIGHT_DP,
            FixtureVariant.RAW,
            expectedGroups = 5,
            expectedBedtime = false,
        )
    }

    // verify unavailable never invents weather or credit
    @Test
    fun testUnavailablePortrait() {
        verifyRenderedWidget(
            FixtureHostActivity.PORTRAIT_WIDTH_DP,
            FixtureHostActivity.PORTRAIT_HEIGHT_DP,
            FixtureVariant.UNAVAILABLE,
            expectedGroups = 0,
            expectedBedtime = false,
        )
    }

    // verify one host resizes and resets reused temperature ink
    @Test
    fun testResizePortraitToLandscape() {
        setOrientation(instrumentation, landscape = true)
        adoptWidgetBinding(instrumentation)
        val intent = Intent(instrumentation.targetContext, FixtureHostActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            putExtra(FixtureHostActivity.EXTRA_WIDTH_DP, FixtureHostActivity.PORTRAIT_WIDTH_DP)
            putExtra(FixtureHostActivity.EXTRA_HEIGHT_DP, FixtureHostActivity.PORTRAIT_HEIGHT_DP)
            putExtra(FixtureHostActivity.EXTRA_VARIANT, FixtureVariant.RAW_MIXED.name)
        }
        val activity = instrumentation.startActivitySync(intent) as FixtureHostActivity
        try {
            val initial = waitForHostView(activity, FixtureVariant.RAW_MIXED, landscape = false)
            // reject the pre-resize fixture as maximum-ready
            assertFalse(FixtureHostActivity.matchesFixture(initial, FixtureVariant.MAXIMUM, landscape = true))
            val mixedGroups = DebugWidgetFixtures.fixture(FixtureVariant.RAW_MIXED).presentation.groups
            assertEquals(listOf("39°", "49°", "65°", "75°", "85°"), mixedGroups.map { it.temperatureLabel })
            assertEquals(
                listOf(
                    WidgetTemperatureTone.COLD,
                    WidgetTemperatureTone.COLD,
                    WidgetTemperatureTone.NEUTRAL,
                    WidgetTemperatureTone.WARM,
                    WidgetTemperatureTone.HOT,
                ),
                mixedGroups.map { it.temperatureTone },
            )
            assertTemperatureColors(
                visibleSlotRoots(initial),
                mixedGroups,
            )
            instrumentation.runOnMainSync {
                activity.resizeWidget(
                    FixtureHostActivity.LANDSCAPE_WIDTH_DP,
                    FixtureHostActivity.LANDSCAPE_HEIGHT_DP,
                    FixtureVariant.MAXIMUM,
                )
            }
            val resized = waitForHostView(activity, FixtureVariant.MAXIMUM, landscape = true)
            // prove reused remoteviews reset warm and hot temperatures to cold and neutral
            val resetGroups = DebugWidgetFixtures.fixture(FixtureVariant.MAXIMUM).presentation.groups
            assertEquals(
                listOf(
                    WidgetTemperatureTone.COLD,
                    WidgetTemperatureTone.COLD,
                    WidgetTemperatureTone.COLD,
                    WidgetTemperatureTone.NEUTRAL,
                    WidgetTemperatureTone.NEUTRAL,
                ),
                resetGroups.map { it.temperatureTone },
            )
            assertTemperatureColors(
                visibleSlotRoots(resized),
                resetGroups,
            )
            assertContentAllocation(
                resized,
                FixtureHostActivity.LANDSCAPE_WIDTH_DP,
                FixtureHostActivity.LANDSCAPE_HEIGHT_DP,
                activity.resources.displayMetrics.density,
            )
        } finally {
            instrumentation.runOnMainSync(activity::finish)
            instrumentation.waitForIdleSync()
            instrumentation.uiAutomation.dropShellPermissionIdentity()
        }
    }

    // verify celsius preserves the same dense geometry
    @Test
    fun testCelsiusPortrait() {
        verifyRenderedWidget(
            FixtureHostActivity.PORTRAIT_WIDTH_DP,
            FixtureHostActivity.PORTRAIT_HEIGHT_DP,
            FixtureVariant.CELSIUS,
            expectedGroups = 5,
            expectedBedtime = false,
        )
    }

    // prove the glyph oracle rejects a clipped descender
    @Test
    fun testGlyphBoundaryDetectorRejectsClippedDescender() {
        val textView = TextView(instrumentation.targetContext).apply {
            text = "5–7p"
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            includeFontPadding = false
            gravity = Gravity.CENTER
            maxLines = 1
        }
        val density = instrumentation.targetContext.resources.displayMetrics.density
        textView.measure(
            MeasureSpec.makeMeasureSpec((80 * density).toInt(), MeasureSpec.EXACTLY),
            MeasureSpec.makeMeasureSpec((10 * density).toInt(), MeasureSpec.EXACTLY),
        )
        textView.layout(0, 0, textView.measuredWidth, textView.measuredHeight)
        assertTrue("clipped descender escaped the glyph oracle", glyphsCrossVerticalBoundary(textView))
    }

    // verify dense typography and visible hour marks across sunset at both heights
    @Test
    fun testDenseSixPanelPortrait() {
        setOrientation(instrumentation, landscape = false)
        val context = instrumentation.targetContext
        val density = context.resources.displayMetrics.density
        val base = DebugWidgetFixtures.fixture(FixtureVariant.MAXIMUM).presentation
        val labels = listOf("Now", "8am", "10am", "12pm", "2pm", "4pm")
        val temperatures = listOf("-12°", "-1°", "0°", "55°", "85°", "99°")
        // repeat semantic conditions while preserving one current panel
        val groups = temperatures.mapIndexed { index, temperature ->
            base.groups[index % base.groups.size].copy(
                isNow = index == 0,
                hourLabel = labels[index],
                landscapeLabel = labels[index],
                accessibleHours = "${labels[index]} dense fixture",
                temperatureLabel = temperature,
            )
        }
        val presentation = base.copy(groups = groups)
        val options = Bundle().apply {
            putInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, DENSE_WIDTH_DP)
            putInt(AppWidgetManager.OPTION_APPWIDGET_MAX_WIDTH, DENSE_WIDTH_DP)
            putInt(AppWidgetManager.OPTION_APPWIDGET_MIN_HEIGHT, FixtureHostActivity.PORTRAIT_HEIGHT_DP)
            putInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT, FixtureHostActivity.PORTRAIT_HEIGHT_DP)
        }
        val remoteViews = WeatherWidgetRenderer.render(context, 0, options, presentation)
        val container = FrameLayout(context)
        val rendered = remoteViews.apply(context, container)
        val widthPx = (DENSE_WIDTH_DP * density).roundToInt()
        val heightPx = (FixtureHostActivity.PORTRAIT_HEIGHT_DP * density).roundToInt()
        container.addView(rendered, FrameLayout.LayoutParams(widthPx, heightPx))
        container.measure(
            MeasureSpec.makeMeasureSpec(widthPx, MeasureSpec.EXACTLY),
            MeasureSpec.makeMeasureSpec(heightPx, MeasureSpec.EXACTLY),
        )
        container.layout(0, 0, widthPx, heightPx)
        val allViews = mutableListOf<View>()
        collectViews(rendered, allViews)
        val slotRoots = visibleSlotRoots(rendered)
        assertEquals(6, slotRoots.size)
        assertEquals(temperatures, slotRoots.map {
            checkNotNull(it.findViewById<TextView>(R.id.slot_temperature)).text.toString()
        })
        assertSlotHierarchy(slotRoots, compact = false)
        assertTemperatureColors(slotRoots, groups)
        assertRowWidths(rendered, slotRoots, presentation)
        assertHourTicks(rendered, presentation, compact = false, context)
        assertVisibleTextWithinHost(rendered, allViews)
        assertNoTextClipping(allViews, compact = false)
        assertNoVisibleFooter(allViews, presentation)
        captureView(rendered, "dense-six-${DENSE_WIDTH_DP}x${FixtureHostActivity.PORTRAIT_HEIGHT_DP}-font1_0-light")
        // cross an interior hour boundary so the fixture contains ticks on both blush surfaces
        val shaded = presentation.copy(sunset = groups[2].start.plusSeconds(5_400))
        // prove composed visibility in both normal and compact rows
        for (heightDp in listOf(FixtureHostActivity.PORTRAIT_HEIGHT_DP, FixtureHostActivity.LANDSCAPE_HEIGHT_DP)) {
            options.putInt(AppWidgetManager.OPTION_APPWIDGET_MIN_HEIGHT, heightDp)
            options.putInt(AppWidgetManager.OPTION_APPWIDGET_MAX_HEIGHT, heightDp)
            val compact = heightDp == FixtureHostActivity.LANDSCAPE_HEIGHT_DP
            val shadedViews = WeatherWidgetRenderer.render(context, 0, options, shaded).apply(context, container)
            val shadedHeightPx = (heightDp * density).roundToInt()
            container.removeAllViews()
            container.addView(shadedViews, FrameLayout.LayoutParams(widthPx, shadedHeightPx))
            container.measure(
                MeasureSpec.makeMeasureSpec(widthPx, MeasureSpec.EXACTLY),
                MeasureSpec.makeMeasureSpec(shadedHeightPx, MeasureSpec.EXACTLY),
            )
            container.layout(0, 0, widthPx, shadedHeightPx)
            assertHourTicks(shadedViews, shaded, compact, context, requireSunsetCoverage = true)
            val shadedChildren = mutableListOf<View>()
            collectViews(shadedViews, shadedChildren)
            assertNoTextClipping(shadedChildren, compact)
            captureView(shadedViews, "sunset-ticks-${DENSE_WIDTH_DP}x${heightDp}")
        }
    }

    // inspect the inflated remoteviews hierarchy
    private fun verifyRenderedWidget(
        widthDp: Int,
        heightDp: Int,
        variant: FixtureVariant,
        expectedGroups: Int,
        expectedBedtime: Boolean,
    ) {
        val landscape = heightDp == FixtureHostActivity.LANDSCAPE_HEIGHT_DP
        setOrientation(instrumentation, landscape)
        adoptWidgetBinding(instrumentation)
        val intent = Intent(instrumentation.targetContext, FixtureHostActivity::class.java).apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            putExtra(FixtureHostActivity.EXTRA_WIDTH_DP, widthDp)
            putExtra(FixtureHostActivity.EXTRA_HEIGHT_DP, heightDp)
            putExtra(FixtureHostActivity.EXTRA_VARIANT, variant.name)
        }
        val activity = instrumentation.startActivitySync(intent) as FixtureHostActivity
        try {
            var hostView = waitForHostView(activity, variant, landscape)
            // force one late real provider update through the flaky cutoff case
            if (variant == FixtureVariant.NEAR_CUTOFF) {
                val priorRestorations = activity.fixtureRestorationCount
                instrumentation.runOnMainSync(activity::requestProviderUpdate)
                waitForFixtureRestoration(activity, priorRestorations)
                hostView = waitForHostView(activity, variant, landscape)
            }
            val allViews = mutableListOf<View>()
            collectViews(hostView, allViews)
            val fixtureDefinition = DebugWidgetFixtures.fixture(variant)
            val fixture = fixtureDefinition.presentation
            val density = activity.resources.displayMetrics.density
            assertContentAllocation(hostView, widthDp, heightDp, density)
            assertRootSurface(hostView, activity)
            val slotRoots = allViews.filter { it.id == R.id.slot_root && it.visibility == View.VISIBLE }
            assertEquals(expectedGroups, slotRoots.size)
            // preserve every forecast interval exactly once in dense fixtures
            if (expectedGroups == 5) {
                assertEquals((0..20).toList(), fixtureDefinition.coveredIntervals)
                assertEquals(listOf("Now", "1am", "4am", "7am", "2pm"), fixture.groups.map { it.hourLabel })
            }
            // bind the first overnight fifth to every 4–8 PM weather interval
            if (variant == FixtureVariant.OVERNIGHT_FIFTH) {
                assertEquals((17..20).toList(), fixtureDefinition.coveredIntervals)
                assertEquals(listOf("Now", "5pm", "6pm", "7pm"), fixture.groups.map { it.hourLabel })
            }
            // bind the near-cutoff fifth to the final 7–8 PM interval
            if (variant == FixtureVariant.NEAR_CUTOFF) {
                assertEquals(listOf(20), fixtureDefinition.coveredIntervals)
                assertEquals(listOf("Now"), fixture.groups.map { it.hourLabel })
            }
            // keep only the leading segment on the white current panel
            if (expectedGroups > 0) {
                assertTrue(fixture.groups.first().isNow)
                assertTrue(fixture.groups.drop(1).none { it.isNow })
            }
            assertTrue(slotRoots.all { it is FrameLayout })
            assertEquals(
                fixture.groups.map { it.accessibilityLabel() },
                slotRoots.map { it.contentDescription?.toString() },
            )
            assertSlotHierarchy(slotRoots, compact = landscape)
            assertTemperatureColors(slotRoots, fixture.groups)
            // assert exact displayed start labels rather than fixture-only values
            val expectedHourLabels = when (expectedGroups) {
                5 -> listOf("Now", "1am", "4am", "7am", "2pm")
                4 -> listOf("Now", "5pm", "6pm", "7pm")
                1 -> listOf("Now")
                else -> emptyList()
            }
            val visibleHourLabels = slotRoots.map { checkNotNull(it.findViewById<TextView>(R.id.slot_hour)).text.toString() }
            assertEquals(expectedHourLabels, visibleHourLabels)
            assertVisibleTextWithinHost(hostView, allViews)
            val bedtimeVisible = allViews.filterIsInstance<TextView>().any { it.text.toString() == "Overnight" }
            assertEquals(expectedBedtime, bedtimeVisible)
            assertMessageHierarchy(allViews, fixture, expectedBedtime, compact = landscape)
            assertRowWidths(hostView, slotRoots, fixture)
            assertPresentationSemantics(hostView, fixture, expectedGroups)
            assertNoVisibleFooter(allViews, fixture)
            assertPanelLayers(hostView, variant, activity)
            assertHourTicks(hostView, fixture, compact = landscape, activity)
            val fontScale = activity.resources.configuration.fontScale.toString().replace('.', '_')
            val nightMode = activity.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
            // name the active day or night palette
            val appearance = if (nightMode == Configuration.UI_MODE_NIGHT_YES) "dark" else "light"
            captureScreenshot(activity, "${variant.name.lowercase()}-${widthDp}x${heightDp}-font${fontScale}-${appearance}")
            assertNoTextClipping(allViews, compact = landscape)
            // prove update entrypoints coalesce named refresh work
            if (variant == FixtureVariant.MAXIMUM && !landscape) {
                val workManager = WorkManager.getInstance(activity)
                val periodic = workManager.getWorkInfosForUniqueWork("weather-widget-periodic-v1")
                    .get(5, TimeUnit.SECONDS).single()
                val immediate = workManager.getWorkInfosForUniqueWork("weather-widget-immediate-v1")
                    .get(5, TimeUnit.SECONDS).single()
                assertEquals(NetworkType.CONNECTED, periodic.constraints.requiredNetworkType)
                assertEquals(TimeUnit.MINUTES.toMillis(30), periodic.periodicityInfo?.repeatIntervalMillis)
                assertEquals(NetworkType.CONNECTED, immediate.constraints.requiredNetworkType)
                assertEquals(null, immediate.periodicityInfo)
            }
        } finally {
            // release the host allocation between cases
            instrumentation.runOnMainSync(activity::finish)
            instrumentation.waitForIdleSync()
            instrumentation.uiAutomation.dropShellPermissionIdentity()
        }
    }

    // separate the requested content size from platform host padding
    private fun assertContentAllocation(hostView: View, widthDp: Int, heightDp: Int, density: Float) {
        val root = checkNotNull(hostView.findViewById<View>(android.R.id.background))
        assertEquals((widthDp * density).toInt(), root.width)
        assertEquals((heightDp * density).toInt(), root.height)
        assertEquals(root.width + hostView.paddingLeft + hostView.paddingRight, hostView.width)
        assertEquals(root.height + hostView.paddingTop + hostView.paddingBottom, hostView.height)
    }

    // wait for appwidgethost to receive provider remoteviews
    private fun waitForHostView(
        activity: FixtureHostActivity,
        variant: FixtureVariant,
        landscape: Boolean,
    ): View {
        var lastText = ""
        // wait through bounded host callbacks
        repeat(50) {
            instrumentation.waitForIdleSync()
            var readyView: View? = null
            // observe one atomic host state
            instrumentation.runOnMainSync {
                val candidate = activity.renderedHostView
                // retain the last hierarchy for actionable failures
                if (candidate != null) {
                    val allViews = mutableListOf<View>()
                    collectViews(candidate, allViews)
                    lastText = allViews.filterIsInstance<TextView>().joinToString(" | ") { it.text }
                }
                // require exact content inside its completed target allocation
                if (candidate != null &&
                    candidate.childCount > 0 &&
                    matchesVariant(candidate, variant, landscape) &&
                    matchesTargetLayout(candidate, landscape)
                ) {
                    readyView = candidate
                }
            }
            // return only the observed ready host
            readyView?.let { return it }
            SystemClock.sleep(100)
        }
        fail("AppWidgetHost did not render expected provider RemoteViews: $lastText")
        error("unreachable")
    }

    // reject the provider's initial maximum update for alternate fixtures
    private fun matchesVariant(view: View, variant: FixtureVariant, landscape: Boolean): Boolean {
        return FixtureHostActivity.matchesFixture(view, variant, landscape)
    }

    // require one attached and fully laid-out widget allocation
    private fun matchesTargetLayout(view: View, landscape: Boolean): Boolean {
        val root = view.findViewById<View>(android.R.id.background) ?: return false
        val density = view.resources.displayMetrics.density
        // select the requested content width
        val expectedWidthDp = if (landscape) {
            FixtureHostActivity.LANDSCAPE_WIDTH_DP
        } else {
            FixtureHostActivity.PORTRAIT_WIDTH_DP
        }
        // select the requested content height
        val expectedHeightDp = if (landscape) {
            FixtureHostActivity.LANDSCAPE_HEIGHT_DP
        } else {
            FixtureHostActivity.PORTRAIT_HEIGHT_DP
        }
        return view.isAttachedToWindow &&
            view.isLaidOut &&
            !view.isLayoutRequested &&
            root.isAttachedToWindow &&
            root.isLaidOut &&
            !root.isLayoutRequested &&
            root.width == (expectedWidthDp * density).toInt() &&
            root.height == (expectedHeightDp * density).toInt()
    }

    // await one observed late-provider correction without timer assumptions
    private fun waitForFixtureRestoration(activity: FixtureHostActivity, priorRestorations: Int) {
        // wait through bounded broadcast and host callbacks
        repeat(50) {
            instrumentation.waitForIdleSync()
            // return only after the real host observed and repaired the overwrite
            if (activity.fixtureRestorationCount > priorRestorations) {
                return
            }
            SystemClock.sleep(100)
        }
        fail("AppWidgetHost did not observe the late provider lifecycle update")
    }

    // verify every full-height weather segment and its native children
    private fun assertSlotHierarchy(slotRoots: List<View>, compact: Boolean) {
        val density = instrumentation.targetContext.resources.displayMetrics.density
        val resources = instrumentation.targetContext.resources
        // inspect each equal-width segment independently
        for (slot in slotRoots) {
            assertEquals(null, slot.background)
            assertEquals(View.IMPORTANT_FOR_ACCESSIBILITY_YES, slot.importantForAccessibility)
            val hour = checkNotNull(slot.findViewById<TextView>(R.id.slot_hour))
            val temperature = checkNotNull(slot.findViewById<TextView>(R.id.slot_temperature))
            val iconSpace = checkNotNull(slot.findViewById<FrameLayout>(R.id.slot_icon_space))
            val condition = checkNotNull(slot.findViewById<ImageView>(R.id.slot_condition))
            val content = checkNotNull(hour.parent as? LinearLayout)
            assertTrue(content.parent === slot)
            assertTrue(iconSpace.parent === content)
            assertTrue(temperature.parent === content)
            assertEquals(LinearLayout.VERTICAL, content.orientation)
            assertEquals(0, content.top)
            assertEquals(slot.height, content.height)
            assertEquals(slot.width, content.width)
            assertEquals(1f, (iconSpace.layoutParams as LinearLayout.LayoutParams).weight, 0f)
            assertEquals(null, condition.contentDescription)
            assertArtworkFillsGap(iconSpace, condition, temperature, compact, density)
            val conditionBitmap = condition.drawable as? BitmapDrawable
            assertEquals(512, checkNotNull(conditionBitmap).bitmap.width)
            assertEquals(512, conditionBitmap.bitmap.height)
            assertTrue(condition.parent === iconSpace)
            // retain exact bundled Google Sans weights
            assertEquals(Typeface.create(resources.getFont(R.font.google_sans_regular), Typeface.NORMAL), hour.typeface)
            assertEquals(Typeface.create(resources.getFont(R.font.google_sans_bold), Typeface.BOLD), temperature.typeface)
            assertTrue(temperature.typeface.isBold)
            assertTrue(abs(hour.textSize / density - if (compact) 10f else 13f) < 0.6f)
            assertTrue(abs(temperature.textSize / density - if (compact) 11.2f else 16.8f) < 0.6f)
            // match left and bottom text edges to the unchanged top inset
            assertMargins(hour, if (compact) 0f else 2f, if (compact) 0f else 2f,
                if (compact) 3f else 6f, 0f, density)
            assertMargins(temperature, if (compact) 0f else 2f, 0f,
                if (compact) 3f else 6f, if (compact) 0f else 2f, density)
            assertCentered(iconSpace, condition, density)
            assertCenteredInTextGap(hour, temperature, condition)
        }
    }

    // compare rendered temperature ink with literal product colors
    private fun assertTemperatureColors(slotRoots: List<View>, groups: List<WidgetGroup>) {
        assertEquals(groups.size, slotRoots.size)
        // inspect every semantic tone in the inflated remoteviews
        for ((slot, group) in slotRoots.zip(groups)) {
            val hour = checkNotNull(slot.findViewById<TextView>(R.id.slot_hour))
            val temperature = checkNotNull(slot.findViewById<TextView>(R.id.slot_temperature))
            assertEquals(Color.rgb(67, 46, 59), hour.currentTextColor)
            // bind tones to requested literal ink instead of resource self-comparison
            val expected = when (group.temperatureTone) {
                WidgetTemperatureTone.NEUTRAL -> Color.rgb(67, 46, 59)
                WidgetTemperatureTone.COLD -> Color.rgb(45, 99, 163)
                WidgetTemperatureTone.WARM -> Color.rgb(172, 80, 14)
                WidgetTemperatureTone.HOT -> Color.rgb(181, 47, 38)
            }
            assertEquals("wrong temperature color for ${group.temperatureTone}", expected, temperature.currentTextColor)
        }
    }

    // collect only visible weather segment roots
    private fun visibleSlotRoots(hostView: View): List<View> {
        val views = mutableListOf<View>()
        collectViews(hostView, views)
        return views.filter { it.id == R.id.slot_root && it.visibility == View.VISIBLE }
    }

    // keep the illustration centered in the real label-to-temperature gap
    private fun assertCenteredInTextGap(hour: TextView, temperature: TextView, condition: ImageView) {
        val iconSpace = checkNotNull(condition.parent as? View)
        val gapTop = hour.bottom
        val gapBottom = temperature.top
        val iconTop = iconSpace.top + condition.top
        val iconBottom = iconTop + condition.height
        assertTrue("weather illustration has no vertical gap", gapBottom > gapTop)
        assertTrue("weather illustration overlaps hour", iconTop >= gapTop)
        assertTrue("weather illustration overlaps temperature", iconBottom <= gapBottom)
        val expectedCenter = (gapTop + gapBottom) / 2f
        val actualCenter = (iconTop + iconBottom) / 2f
        assertTrue("weather illustration is not gap-centered", abs(actualCenter - expectedCenter) <= 1f)
    }

    // keep hour text anchored at the segment's upper-left corner
    private fun assertTopStart(view: TextView) {
        val layoutGravity = (view.layoutParams as FrameLayout.LayoutParams).gravity
        val gravity = Gravity.getAbsoluteGravity(layoutGravity, view.layoutDirection)
        assertEquals(Gravity.TOP, gravity and Gravity.VERTICAL_GRAVITY_MASK)
        assertEquals(Gravity.LEFT, gravity and Gravity.HORIZONTAL_GRAVITY_MASK)
    }

    // keep temperature anchored at the segment's lower-left corner
    private fun assertBottomStart(view: TextView) {
        val layoutGravity = (view.layoutParams as FrameLayout.LayoutParams).gravity
        val gravity = Gravity.getAbsoluteGravity(layoutGravity, view.layoutDirection)
        assertEquals(Gravity.BOTTOM, gravity and Gravity.VERTICAL_GRAVITY_MASK)
        assertEquals(Gravity.LEFT, gravity and Gravity.HORIZONTAL_GRAVITY_MASK)
    }

    // keep the condition illustration centered in its segment
    private fun assertCentered(parent: View, child: View, density: Float) {
        assertTrue(child.parent === parent)
        val parentCenterX = parent.width / 2f
        val parentCenterY = parent.height / 2f
        val childCenterX = child.left + child.width / 2f
        val childCenterY = child.top + child.height / 2f
        assertTrue("condition is not horizontally centered", abs(parentCenterX - childCenterX) <= 2f * density)
        assertTrue("condition is not vertically centered", abs(parentCenterY - childCenterY) <= 2f * density)
    }

    // prove the bitmap itself grows uniformly rather than only widening its empty container
    private fun assertArtworkFillsGap(space: View, icon: ImageView, temperature: TextView, compact: Boolean, density: Float) {
        assertEquals(space.width, icon.width)
        assertEquals(space.height, icon.height)
        assertEquals(0, icon.paddingLeft)
        assertEquals(0, icon.paddingTop)
        assertEquals(0, icon.paddingRight)
        assertEquals(0, icon.paddingBottom)
        assertEquals(ImageView.ScaleType.FIT_CENTER, icon.scaleType)
        val artwork = RectF(checkNotNull(icon.drawable).bounds)
        icon.imageMatrix.mapRect(artwork)
        val available = minOf(space.width, space.height).toFloat()
        assertEquals(available, artwork.width(), 1f)
        assertEquals(available, artwork.height(), 1f)
        // measure the former temperature to prove actual growth from the reclaimed text height
        val priorTemperature = TextView(icon.context).apply {
            typeface = temperature.typeface
            includeFontPadding = false
            maxLines = 1
            text = temperature.text
            setTextSize(TypedValue.COMPLEX_UNIT_DIP, if (compact) 16f else 24f)
            measure(MeasureSpec.UNSPECIFIED, MeasureSpec.UNSPECIFIED)
        }
        val reclaimedHeight = priorTemperature.measuredHeight - temperature.height
        assertTrue("temperature reduction did not free icon height", reclaimedHeight > 0)
        val priorArtworkSize = minOf(space.width, space.height - reclaimedHeight).toFloat()
        // narrow overnight panels can fill their width before using all reclaimed height
        assertTrue(
            "artwork did not grow: ${priorArtworkSize / density}dp to ${artwork.width() / density}dp",
            artwork.width() - priorArtworkSize >= density,
        )
        assertTrue(artwork.left >= -1f && artwork.top >= -1f)
        assertTrue(artwork.right <= icon.width + 1f && artwork.bottom <= icon.height + 1f)
    }

    // compare one linear-layout child's logical margins in dp
    private fun assertMargins(
        view: View,
        startDp: Float,
        topDp: Float,
        endDp: Float,
        bottomDp: Float,
        density: Float,
    ) {
        val margins = view.layoutParams as LinearLayout.LayoutParams
        assertTrue(abs(margins.marginStart / density - startDp) < 0.6f)
        assertTrue(abs(margins.topMargin / density - topDp) < 0.6f)
        assertTrue(abs(margins.marginEnd / density - endDp) < 0.6f)
        assertTrue(abs(margins.bottomMargin / density - bottomDp) < 0.6f)
    }

    // verify the remaining row is one native message segment
    private fun assertMessageHierarchy(
        views: List<View>,
        presentation: farm.ballydidean.weather.widget.WidgetPresentation,
        expectedBedtime: Boolean,
        compact: Boolean,
    ) {
        val roots = views.filter { it.id == R.id.bedtime_root && it.visibility == View.VISIBLE }
        assertEquals(if (presentation.message == null) 0 else 1, roots.size)
        // stop when the forecast fills the complete row
        if (presentation.message == null) {
            return
        }
        val root = roots.single()
        assertTrue(root is FrameLayout)
        assertEquals(null, root.background)
        val hour = checkNotNull(root.findViewById<TextView>(R.id.bedtime_hour))
        val message = checkNotNull(root.findViewById<TextView>(R.id.bedtime_message))
        val icon = checkNotNull(root.findViewById<ImageView>(R.id.bedtime_icon))
        val content = checkNotNull(hour.parent as? LinearLayout)
        assertTrue(content.parent === root)
        assertTrue(message.parent === content)
        assertEquals(LinearLayout.VERTICAL, content.orientation)
        assertEquals(root.height, content.height)
        assertEquals(root.width, content.width)
        assertEquals(hour.left, message.left)
        assertTrue(hour.top < message.top)
        val regular = Typeface.create(instrumentation.targetContext.resources.getFont(R.font.google_sans_regular), Typeface.NORMAL)
        assertEquals(regular, hour.typeface)
        assertEquals(Typeface.create(instrumentation.targetContext.resources.getFont(R.font.google_sans_bold), Typeface.BOLD), message.typeface)
        assertEquals(if (expectedBedtime) "Overnight" else "", hour.text.toString())
        assertEquals(presentation.message, message.text.toString())
        assertEquals(if (expectedBedtime) View.VISIBLE else View.GONE, icon.visibility)
        val density = instrumentation.targetContext.resources.displayMetrics.density
        // retain the dedicated trailing room on the overnight label
        assertMargins(hour, if (compact) 0f else 2f, if (compact) 0f else 2f,
            if (compact) 3f else 4f, 0f, density)
        assertMargins(message, if (compact) 0f else 2f, 0f,
            if (compact) 3f else 6f, if (compact) 0f else 2f, density)
        assertTrue(abs(message.textSize / density - if (compact) 11.2f else 16.8f) < 0.6f)
        // center the summary icon between the label and bold low even in compact rows
        if (expectedBedtime) {
            val space = checkNotNull(root.findViewById<View>(R.id.bedtime_icon_space))
            assertCentered(space, icon, instrumentation.targetContext.resources.displayMetrics.density)
            assertCenteredInTextGap(hour, message, icon)
            assertArtworkFillsGap(space, icon, message, compact, density)
            val iconBitmap = icon.drawable as? BitmapDrawable
            assertEquals(512, checkNotNull(iconBitmap).bitmap.width)
            assertEquals(512, iconBitmap.bitmap.height)
            assertEquals(instrumentation.targetContext.getColor(R.color.widget_temperature_cold), message.currentTextColor)
        }
        assertEquals(
            if (expectedBedtime) presentation.overnight?.accessibilityLabel() else presentation.message,
            root.contentDescription?.toString(),
        )
    }

    // require equal weather fifths and one remainder-filling message
    private fun assertRowWidths(
        hostView: View,
        slotRoots: List<View>,
        presentation: farm.ballydidean.weather.widget.WidgetPresentation,
    ) {
        val row = checkNotNull(hostView.findViewById<ViewGroup>(R.id.row_primary))
        val root = checkNotNull(hostView.findViewById<View>(android.R.id.background))
        assertEquals(root.height, row.height)
        val weatherWidth = row.width / maxOf(5, presentation.groups.size).toFloat()
        var priorRight = 0
        // keep every weather group full-height contiguous and at most one fifth
        for (slot in slotRoots) {
            assertEquals(0, slot.top)
            assertEquals(row.height, slot.height)
            assertTrue("unequal weather width", abs(slot.width - weatherWidth) <= 1.5f)
            assertTrue("weather segment exceeds one fifth", slot.width <= row.width * 0.2f + 1.5f)
            assertTrue("weather segments are not contiguous", abs(slot.left - priorRight) <= 1)
            priorRight = slot.right
        }
        val message = row.findViewById<View>(R.id.bedtime_root)
        // reserve every unused fifth for the sole message
        if (presentation.message != null) {
            val expected = row.width - presentation.groups.size * weatherWidth
            val messageView = checkNotNull(message)
            assertEquals(0, messageView.top)
            assertEquals(row.height, messageView.height)
            assertTrue("message does not start after weather", abs(messageView.left - priorRight) <= 1)
            assertTrue("message does not fill the row remainder", abs(messageView.width - expected) <= 1.5f)
            assertEquals(row.width, messageView.right)
        } else {
            assertEquals(null, message)
            assertEquals(row.width, priorRight)
        }
    }

    // retain metadata in one accessible root without rendering a footer
    private fun assertPresentationSemantics(
        hostView: View,
        presentation: farm.ballydidean.weather.widget.WidgetPresentation,
        expectedGroups: Int,
    ) {
        val root = checkNotNull(hostView.findViewById<View>(android.R.id.background))
        val description = root.contentDescription?.toString().orEmpty()
        assertTrue(description.contains("${presentation.status.name.lowercase()} forecast"))
        assertTrue(description.contains(if (presentation.stale) "stale" else "current"))
        assertTrue(description.contains(
            if (presentation.unit == farm.ballydidean.weather.widget.TemperatureUnit.FAHRENHEIT) {
                "degrees Fahrenheit"
            } else {
                "degrees Celsius"
            },
        ))
        // keep attribution accessible only with rendered weather
        if (presentation.showCredit) {
            assertTrue(description.contains("Open-Meteo, CC BY 4.0"))
        } else {
            assertFalse(description.contains("Open-Meteo"))
        }
        // expose the visible fixture sunset through root semantics
        if (presentation.sunset != null) {
            assertTrue(description.contains("Sunset 7:30 PM"))
        } else {
            assertFalse(description.contains("Sunset"))
        }
    }

    // reject visible status credit unit or footer text
    private fun assertNoVisibleFooter(
        views: List<View>,
        presentation: farm.ballydidean.weather.widget.WidgetPresentation,
    ) {
        val visibleValues = views.filterIsInstance<TextView>()
            .filter { it.visibility == View.VISIBLE && it.text.isNotEmpty() }
            .map { it.text.toString() }
        // allow only forecast labels and the optional row message
        val expectedValues = presentation.groups.flatMap { listOf(it.hourLabel, it.temperatureLabel) } +
            if (presentation.message == null) {
                emptyList()
            } else if (presentation.showBedtime) {
                listOf("Overnight", presentation.message)
            } else {
                listOf(presentation.message)
            }
        assertEquals(expectedValues.sorted(), visibleValues.sorted())
        val visibleText = visibleValues.joinToString(" ")
        assertFalse(visibleText.contains("Open-Meteo"))
        assertFalse(visibleText.contains("CC BY 4.0"))
        assertFalse(visibleText.contains("°F"))
        assertFalse(visibleText.contains("°C"))
        assertFalse(visibleText.contains("stale"))
        assertFalse(visibleText.contains("adjusted"))
        assertFalse(visibleText.contains("mixed"))
        assertFalse(visibleText.contains("raw"))
    }

    // verify one clipped rounded root with no sunset overlay
    private fun assertRootSurface(hostView: View, activity: FixtureHostActivity) {
        val root = checkNotNull(hostView.findViewById<ViewGroup>(android.R.id.background))
        assertTrue(root.clipToOutline)
        val background = checkNotNull(root.background as? GradientDrawable)
        assertTrue(abs(background.cornerRadius - 8f * activity.resources.displayMetrics.density) <= 0.6f)
        assertEquals(3, root.childCount)
        assertEquals(R.id.widget_panels, root.getChildAt(0).id)
        assertEquals(R.id.widget_hour_ticks, root.getChildAt(1).id)
        assertEquals(R.id.row_primary, root.getChildAt(2).id)
    }

    // verify current forecast evening and bedtime panel colors
    private fun assertPanelLayers(hostView: View, variant: FixtureVariant, activity: FixtureHostActivity) {
        val panels = checkNotNull(hostView.findViewById<ImageView>(R.id.widget_panels))
        val panelBitmap = checkNotNull((panels.drawable as? BitmapDrawable)?.bitmap) {
            "widget panel bitmap is unavailable"
        }
        // lock the requested white blush and blue surface palette
        assertEquals(Color.WHITE, activity.getColor(R.color.widget_now))
        assertEquals(Color.rgb(248, 222, 229), activity.getColor(R.color.widget_background))
        assertEquals(Color.rgb(234, 184, 200), activity.getColor(R.color.widget_after_sunset))
        assertEquals(Color.rgb(218, 234, 245), activity.getColor(R.color.widget_bedtime))
        // verify white now light daytime and fractional dark post-sunset forecast
        if (variant == FixtureVariant.MAXIMUM) {
            assertEquals(activity.getColor(R.color.widget_now), panelBitmap.getPixel(panelBitmap.width / 10, 0))
            assertEquals(activity.getColor(R.color.widget_background), panelBitmap.getPixel(panelBitmap.width * 3 / 10, 0))
            val shadeBoundary = panelBitmap.width * 59 / 60
            val offset = maxOf(2, activity.resources.displayMetrics.density.toInt() + 1)
            assertEquals(
                activity.getColor(R.color.widget_background),
                panelBitmap.getPixel(maxOf(0, shadeBoundary - offset), 0),
            )
            assertEquals(
                activity.getColor(R.color.widget_after_sunset),
                panelBitmap.getPixel(minOf(panelBitmap.width - 1, shadeBoundary + offset), 0),
            )
        }
        // divide the 4 PM row into four weather fifths and one blue overnight fifth
        if (variant == FixtureVariant.OVERNIGHT_FIFTH) {
            assertEquals(activity.getColor(R.color.widget_now), panelBitmap.getPixel(panelBitmap.width / 10, 0))
            assertEquals(activity.getColor(R.color.widget_background), panelBitmap.getPixel(panelBitmap.width * 3 / 10, 0))
            assertEquals(activity.getColor(R.color.widget_background), panelBitmap.getPixel(panelBitmap.width * 13 / 20, 0))
            assertEquals(activity.getColor(R.color.widget_after_sunset), panelBitmap.getPixel(panelBitmap.width * 3 / 4, 0))
            assertEquals(activity.getColor(R.color.widget_bedtime), panelBitmap.getPixel(panelBitmap.width * 9 / 10, 0))
        }
        // keep a sunset inside now white while the remaining four fifths stay blue
        if (variant == FixtureVariant.NEAR_CUTOFF) {
            assertEquals(activity.getColor(R.color.widget_now), panelBitmap.getPixel(panelBitmap.width / 10, 0))
            assertEquals(activity.getColor(R.color.widget_bedtime), panelBitmap.getPixel(panelBitmap.width / 2, 0))
        }
    }

    // verify decorative hour marks stay short and proportionally inside their segments
    private fun assertHourTicks(
        hostView: View,
        presentation: farm.ballydidean.weather.widget.WidgetPresentation,
        compact: Boolean,
        context: Context,
        requireSunsetCoverage: Boolean = false,
    ) {
        val density = context.resources.displayMetrics.density
        val root = checkNotNull(hostView.findViewById<ViewGroup>(android.R.id.background))
        val tickView = checkNotNull(hostView.findViewById<ImageView>(R.id.widget_hour_ticks))
        assertEquals(null, tickView.contentDescription)
        assertEquals(View.IMPORTANT_FOR_ACCESSIBILITY_NO, tickView.importantForAccessibility)
        assertEquals(ImageView.ScaleType.FIT_XY, tickView.scaleType)
        assertEquals(0, tickView.top)
        assertEquals(root.width, tickView.width)
        assertTrue(abs(tickView.height / density - 3f) < 0.6f)
        val bitmap = checkNotNull((tickView.drawable as? BitmapDrawable)?.bitmap)
        assertEquals(root.width, bitmap.width)
        assertEquals((3f * density).roundToInt(), bitmap.height)
        val pixels = IntArray(bitmap.width * bitmap.height)
        bitmap.getPixels(pixels, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
        val visibleColumns = BooleanArray(bitmap.width)
        val maximumColumnAlpha = IntArray(bitmap.width)
        val expectedInk = Color.rgb(118, 85, 104)
        var maximumVisibleRow = -1
        // accept antialiased secondary ink inside the tiny transparent strip
        for (index in pixels.indices) {
            val color = pixels[index]
            // record only painted tick pixels
            if (Color.alpha(color) > 0) {
                val alpha = Color.alpha(color)
                val channelTolerance = maxOf(2, kotlin.math.ceil(255.0 / alpha).toInt() + 1)
                assertTrue(abs(Color.red(color) - Color.red(expectedInk)) <= channelTolerance)
                assertTrue(abs(Color.green(color) - Color.green(expectedInk)) <= channelTolerance)
                assertTrue(abs(Color.blue(color) - Color.blue(expectedInk)) <= channelTolerance)
                val column = index % bitmap.width
                visibleColumns[column] = true
                maximumColumnAlpha[column] = maxOf(maximumColumnAlpha[column], alpha)
                maximumVisibleRow = maxOf(maximumVisibleRow, index / bitmap.width)
            }
        }
        val expectedFractions = expectedHourTickFractions(presentation)
        val runs = contiguousRuns(visibleColumns)
        assertEquals(expectedFractions.size, runs.size)
        // keep each mark approximately one dp wide and at its expected elapsed-hour position
        for ((expectedFraction, run) in expectedFractions.zip(runs)) {
            val center = (run.first + run.last + 1) / 2.0
            assertTrue(abs(center - expectedFraction * bitmap.width) <= density)
            assertTrue(run.last - run.first + 1 <= density.roundToInt() + 1)
            assertTrue(run.any { maximumColumnAlpha[it] >= 240 })
        }
        // keep the compact marks shorter while leaving the remainder transparent
        if (runs.isEmpty()) {
            assertEquals(-1, maximumVisibleRow)
        } else {
            val paintedHeightLimit = kotlin.math.ceil((if (compact) 2f else 3f) * density).toInt()
            assertTrue(maximumVisibleRow < paintedHeightLimit)
            // reject accidental full-height dividers in the dedicated overlay
            assertTrue(maximumVisibleRow < root.height - 1)
        }
        // inspect the real composition rather than only the transparent source bitmap
        if (requireSunsetCoverage) {
            val panels = checkNotNull(root.findViewById<ImageView>(R.id.widget_panels))
            val panelBitmap = checkNotNull((panels.drawable as? BitmapDrawable)?.bitmap)
            val composed = Bitmap.createBitmap(root.width, bitmap.height, Bitmap.Config.ARGB_8888)
            instrumentation.runOnMainSync { root.draw(Canvas(composed)) }
            var daytimeTicks = 0
            var sunsetTicks = 0
            val sampleY = minOf(1, bitmap.height - 1)
            // require each fully painted tick to remain visible over its actual panel background
            for (run in runs) {
                val x = run.maxBy { Color.alpha(bitmap.getPixel(it, sampleY)) }
                val background = panelBitmap.getPixel(x, 0)
                assertEquals("hour tick covered at x=$x", expectedInk, composed.getPixel(x, sampleY))
                // prove the synthetic fixture exercises both sides of sunset
                when (background) {
                    Color.rgb(248, 222, 229) -> daytimeTicks += 1
                    Color.rgb(234, 184, 200) -> sunsetTicks += 1
                }
            }
            assertTrue("fixture has no daytime ticks", daytimeTicks > 0)
            assertTrue("fixture has no post-sunset ticks", sunsetTicks > 0)
        }
    }

    // derive an independent normalized oracle for every visible segment
    private fun expectedHourTickFractions(
        presentation: farm.ballydidean.weather.widget.WidgetPresentation,
    ): List<Double> {
        val weatherFraction = 1.0 / maxOf(5, presentation.groups.size)
        val expected = mutableListOf<Double>()
        // omit current and map only interior forecast hours into the complete row
        for ((index, group) in presentation.groups.withIndex()) {
            // current retains no unlabeled ticks
            if (!group.isNow) {
                expected += interiorElapsedHourFractions(group.start, group.end)
                    .map { (index + it) * weatherFraction }
            }
        }
        val start = presentation.bedtimeStart
        val end = presentation.bedtimeEnd
        // mark overnight only when its blue remainder is visible with known bounds
        if (presentation.showBedtime && start != null && end != null) {
            val left = presentation.groups.size * weatherFraction
            val width = 1.0 - left
            expected += interiorElapsedHourFractions(start, end).map { left + it * width }
        }
        return expected.sorted()
    }

    // calculate elapsed-hour fractions without reusing production geometry
    private fun interiorElapsedHourFractions(start: Instant, end: Instant): List<Double> {
        val durationMillis = Duration.between(start, end).toMillis()
        val hourMillis = Duration.ofHours(1).toMillis()
        // reject single-hour and invalid ranges
        if (durationMillis <= hourMillis) return emptyList()
        val fractions = mutableListOf<Double>()
        var elapsed = hourMillis
        // retain every strictly interior real hour
        while (elapsed < durationMillis) {
            fractions += elapsed.toDouble() / durationMillis
            elapsed += hourMillis
        }
        return fractions
    }

    // collapse painted bitmap columns into distinct one-dp marks
    private fun contiguousRuns(columns: BooleanArray): List<IntRange> {
        val runs = mutableListOf<IntRange>()
        var start = -1
        // inspect every column plus one terminal sentinel
        for (index in 0..columns.size) {
            val visible = index < columns.size && columns[index]
            // begin one new painted run
            if (visible && start < 0) {
                start = index
            }
            // close the active run at the first transparent column
            if (!visible && start >= 0) {
                runs += start until index
                start = -1
            }
        }
        return runs
    }

    // flatten the host hierarchy for semantic inspection
    private fun collectViews(view: View, destination: MutableList<View>) {
        destination += view
        // visit every inflated child
        if (view is ViewGroup) {
            for (index in 0 until view.childCount) {
                collectViews(view.getChildAt(index), destination)
            }
        }
    }

    // reject ellipsized or visibly clipped text and undersized row labels
    private fun assertNoTextClipping(views: List<View>, compact: Boolean) {
        val density = instrumentation.targetContext.resources.displayMetrics.density
        val textViews = views.filterIsInstance<TextView>().filter { it.visibility == View.VISIBLE && it.text.isNotEmpty() }
        // inspect every visible text node
        for (textView in textViews) {
            val layout = textView.layout
            // reject incomplete line layout
            if (layout != null) {
                assertEquals("wrapped text: ${textView.text}", 1, layout.lineCount)
                // inspect the full line without hiding clipped trailing glyphs
                for (line in 0 until layout.lineCount) {
                    assertEquals("ellipsized: ${textView.text}", 0, layout.getEllipsisCount(line))
                    val availableWidth = textView.width - textView.paddingLeft - textView.paddingRight
                    assertTrue(
                        "horizontally clipped: ${textView.text}",
                        layout.getLineWidth(line) <= availableWidth + 0.5f,
                    )
                }
            }
            assertTrue(
                "vertically clipped glyph: ${textView.text} bounds=${glyphBounds(textView)} height=${textView.height}",
                !glyphsCrossVerticalBoundary(textView),
            )
            val minimumDp = when (textView.id) {
                R.id.slot_temperature -> if (compact) 11.2f else 16.8f
                R.id.slot_hour -> if (compact) 10f else 13f
                // allow the long overnight label to fit one fifth panel
                R.id.bedtime_hour -> 10f
                R.id.bedtime_message -> if (compact) 11.2f else 16.8f
                else -> 10f
            }
            assertTrue(
                "undersized text: ${textView.text} size=${textView.textSize / density}dp minimum=${minimumDp}dp",
                textView.textSize / density >= minimumDp - 0.1f,
            )
            assertTrue("zero-size text: ${textView.text}", textView.width > 0 && textView.height > 0)
        }
    }

    // compare real font outlines with the allocated text rectangle
    private fun glyphsCrossVerticalBoundary(textView: TextView): Boolean {
        val bounds = glyphBounds(textView) ?: return false
        return bounds.top < 1f || bounds.bottom > textView.height - 1f
    }

    // combine every real glyph outline in local view coordinates
    private fun glyphBounds(textView: TextView): RectF? {
        val layout = textView.layout ?: return null
        val value = textView.text.toString()
        var combined: RectF? = null
        // inspect every laid-out line outline
        for (line in 0 until layout.lineCount) {
            val path = Path()
            textView.paint.getTextPath(
                value,
                layout.getLineStart(line),
                layout.getLineEnd(line),
                layout.getLineLeft(line) + textView.totalPaddingLeft,
                layout.getLineBaseline(line).toFloat() + textView.totalPaddingTop,
                path,
            )
            val bounds = RectF()
            path.computeBounds(bounds, true)
            // retain only visible glyph geometry
            if (!bounds.isEmpty) {
                val existing = combined
                // merge later line bounds into the first
                if (existing == null) {
                    combined = bounds
                } else {
                    existing.union(bounds)
                }
            }
        }
        return combined
    }

    // keep every visible text rectangle inside the real host allocation
    private fun assertVisibleTextWithinHost(hostView: View, views: List<View>) {
        val hostLocation = IntArray(2).also(hostView::getLocationOnScreen)
        val hostLeft = hostLocation[0]
        val hostTop = hostLocation[1]
        val hostRight = hostLeft + hostView.width
        val hostBottom = hostTop + hostView.height
        val textViews = views.filterIsInstance<TextView>().filter { it.visibility == View.VISIBLE && it.text.isNotEmpty() }
        // inspect every visible text rectangle
        for (textView in textViews) {
            val location = IntArray(2).also(textView::getLocationOnScreen)
            assertTrue("left overflow: ${textView.text}", location[0] >= hostLeft)
            assertTrue("top overflow: ${textView.text}", location[1] >= hostTop)
            assertTrue("right overflow: ${textView.text}", location[0] + textView.width <= hostRight)
            assertTrue("bottom overflow: ${textView.text}", location[1] + textView.height <= hostBottom)
            assertTextWithinClippingAncestors(hostView, textView, location)
        }
    }

    // keep each text rectangle inside every clipping parent
    private fun assertTextWithinClippingAncestors(hostView: View, textView: TextView, textLocation: IntArray) {
        val textRight = textLocation[0] + textView.width
        val textBottom = textLocation[1] + textView.height
        var ancestor = textView.parent as? View
        // inspect parents through the real host boundary
        while (ancestor != null) {
            val location = IntArray(2).also(ancestor::getLocationOnScreen)
            val ancestorRight = location[0] + ancestor.width
            val ancestorBottom = location[1] + ancestor.height
            assertTrue(
                "ancestor left clip: ${textView.text} text=${textLocation[0]} parent=${location[0]}",
                textLocation[0] >= location[0],
            )
            assertTrue("ancestor top clip: ${textView.text}", textLocation[1] >= location[1])
            assertTrue(
                "ancestor right clip: ${textView.text} text=$textRight parent=$ancestorRight",
                textRight <= ancestorRight,
            )
            assertTrue("ancestor bottom clip: ${textView.text}", textBottom <= ancestorBottom)
            // stop after checking the allocation boundary
            if (ancestor === hostView) {
                break
            }
            ancestor = ancestor.parent as? View
        }
    }

    // preserve host evidence inside app external cache
    private fun captureScreenshot(activity: FixtureHostActivity, name: String) {
        val bitmap: Bitmap = instrumentation.uiAutomation.takeScreenshot()
        val directory = File(activity.externalCacheDir, "widget-host-evidence").apply { mkdirs() }
        FileOutputStream(File(directory, "$name.png")).use { stream ->
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, stream)
        }
    }

    // preserve one tightly cropped remoteviews layout artifact
    private fun captureView(view: View, name: String) {
        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap).apply { drawColor(Color.TRANSPARENT) }
        view.draw(canvas)
        val cache = checkNotNull(instrumentation.targetContext.externalCacheDir)
        val directory = File(cache, "widget-host-evidence").apply { mkdirs() }
        FileOutputStream(File(directory, "$name.png")).use { stream ->
            bitmap.compress(Bitmap.CompressFormat.PNG, 100, stream)
        }
    }

    companion object {
        private const val DENSE_WIDTH_DP = 384

        // set the actual device orientation for the requested minimum
        private fun setOrientation(instrumentation: Instrumentation, landscape: Boolean) {
            // choose portrait or landscape rotation
            val rotation = if (landscape) 1 else 0
            val fixedRotation = instrumentation.uiAutomation.executeShellCommand("wm fixed-to-user-rotation enabled")
            ParcelFileDescriptor.AutoCloseInputStream(fixedRotation).use { it.readBytes() }
            val userRotation = instrumentation.uiAutomation.executeShellCommand("wm user-rotation lock $rotation")
            ParcelFileDescriptor.AutoCloseInputStream(userRotation).use { it.readBytes() }
            SystemClock.sleep(1_500)
        }

        // adopt test-only binding authority for binder calls
        private fun adoptWidgetBinding(instrumentation: Instrumentation) {
            instrumentation.uiAutomation.adoptShellPermissionIdentity(android.Manifest.permission.BIND_APPWIDGET)
        }
    }
}
