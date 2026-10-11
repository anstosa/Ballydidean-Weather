# Android Weather companion and widget

This credential-free Android project contains the production hosted `WebView`
shell and one native Ballydidean forecast `AppWidgetProvider`. The widget
strictly decodes `weather-widget/v3` (and legacy v1/v2 caches), renders current cached semantics, and
refreshes through a cookie-free fixed public endpoint. It is not signed for or
submitted to Google Play.

## Hosted OTA updates

The existing Android WebView receives web UI releases without reinstalling the
APK. On startup and foreground, the hosted updater checks the stable service
worker URL. New releases download in the background with a persistent progress
prompt. After the complete public shell and its executable checksums are
verified, **Reload to apply** activates the waiting release. Failed downloads
leave the current shell intact and offer **Retry download**. Cookies and local
preferences are not cleared. Closing every old WebView can also activate a
waiting release under the normal service-worker lifecycle.

This is hosted web OTA, not native APK patching or Capgo. Widget, permission,
and other native changes still need an Android release. See
[the OTA runbook](../../docs/operations/android-ota-updates.md).

## Pinned toolchain

- Android Gradle Plugin 9.4.0 with built-in Kotlin 2.2.10
- Gradle 9.6.0 through the checked wrapper
- JDK 17
- WorkManager `work-runtime` 2.11.2
- compile SDK 37.0, target SDK 36, minimum SDK 26
- Android Build Tools 36.0.0
- Android 36 AOSP x86_64 managed-device image

Load the repository-local defaults before Gradle commands:

```bash
source mobile/android/scripts/android-env.sh
mobile/android/scripts/verify-wrapper.sh
mobile/android/scripts/toolchain-receipt.sh
```

## Hosted shell boundary

The shell always starts at `https://weather.ballydidean.farm`; the only native
route is the enum-like `/forecast` widget action. Exact-origin HTTPS navigation
stays in the `WebView`. Credentialed, lookalike, non-default-port, cleartext,
file, content, JavaScript and malformed URLs are rejected. Other valid HTTPS
links are delegated to the system browser. Popups are rejected, TLS errors are
cancelled, back follows web history, and the retry view can reload only the
canonical origin.

Default persistent first-party `WebView` cookies retain the hosted login and
settings experience. Third-party cookies, file/content access, mixed content
and JavaScript/native bridges are disabled. Widget HTTP/storage code never
reads the `WebView` cookie store. Debug workers do not make production requests
unless live widget testing is explicitly enabled.

For manual testing against the deployed public widget API, add
`weatherLiveWidgetRefresh=true` to the local user's Gradle properties
(`%USERPROFILE%\.gradle\gradle.properties` on Windows or
`~/.gradle/gradle.properties` on Linux), then rebuild and install the app.
The equivalent one-build flag is `-PweatherLiveWidgetRefresh=true`. Keep this
setting outside Git. Release builds always refresh; ordinary debug and CI
builds remain offline by default. The hosted fixture wrapper explicitly
disables live refresh, even when a developer has opted in locally. A missing
production widget endpoint cannot be repaired by repeated refresh attempts.

## Single-row widget design

Weather illustrations use the supplied colors and artwork without a shadow,
tint or other effect. Normal and compact artwork fills the largest square that
fits between the time and temperature, without the former 40dp/20dp caps.
The illustration stays centered and scales uniformly without cropping.
The hour's left inset and the temperature's left and bottom insets match the
unchanged hour top inset: 2dp normally and zero in compact rows. Right insets
remain unchanged. Bold temperatures are 16.8dp/11.2dp, 30% smaller than the
previous sizes, so the illustrations fill the freed space. Shorter widget heights
scale the illustration to the remaining space. The images have no extra padding
or inset box.

The widget has one full-height row, no inset cards and no visible footer.
Multi-hour forecast and overnight segments have short, unlabeled top ticks at
interior hour boundaries; single-hour and Now segments have none. The existing
segment edges mark each range's start and end. Ticks use actual elapsed hours,
including overnight daylight-saving transitions, without changing row height.
Ticks use darker secondary ink so they remain visible above post-sunset blush;
full-height dividers retain their lighter color.
Now is white, forecasts are light blush, and overnight is light blue.
Vertical dividers separate panels. Post-sunset portions of forecast panels are
darker blush, split proportionally within each time block; Now stays white and
overnight stays blue. There is no sunset line. Each weather panel is at most 20% of the width. Overnight
first appears at 4pm with Now, 5pm, 6pm and 7pm each occupying one fifth.
It fills all remaining width as those daytime hours pass.

Now covers the current hour. Future blocks expand one hour at a time until
all hours through 8pm fit, using a 64dp target panel width and at least five
slots. A 384dp or wider row can hold six weather panels. Time is top-left, a
native weather illustration is vertically centered
between the label and temperature, and one bold temperature is bottom-left.
The selected temperature is the block high
above a 65°F mean, the low below a 50°F mean, otherwise the mean. Selection
happens before display-unit conversion and rounding.

The blue remainder tile is labeled `Overnight` and shows the minimum selected
air temperature across 8pm–7am the next morning, not the daytime representative
temperature. Its weather icon summarizes that entire window using the same
rain/cloud/wind rules. Clear and partly cloudy night conditions use the supplied
moon variants; cloud and rain art is shared. Regular post-sunset tiles use moon
variants too. The overnight panel remains visible alone after 8pm until 7am.
Before 7am the feed anchors the preceding evening, including across midnight
and DST. Missing temperature in any member hour shows `—`; incomplete weather
inputs show the unavailable icon, never an invented clear night.

Temperature colors reuse the forecast bands without green: below 55°F is blue,
55–70°F inclusive keeps the existing dark ink, above 70–80°F inclusive is orange,
and above 80°F is red. The blue/orange/red text shades are darkened for readability
on the evening blush background. Color uses the same unrounded selected value
as the displayed temperature and stays unchanged when switching units. Missing
temperature stays neutral; labels, icons and panel colors do not change.

Icons use the wettest selected rain rate (>0 and <2.5mm/h light, >=2.5 heavy),
otherwise mean cloud coverage (<25% sunny, <75% partly cloudy, else cloudy).
Wind variants activate at a selected speed of at least 20mph in any member hour.
Missing input is not presented as sunny or calm. Adjusted values retain their
original deadlines and raw fallbacks.

The native icon set uses the supplied `weather-icons-handoff (3).zip` artwork without
redrawing or recoloring it. Unchanged transparent 512px PNGs live in
`app/src/main/res/drawable-nodpi` under the existing resource names. Native
ImageViews scale them with `fitCenter`; the widget is not a web wrapper.
SVG masters, the original handoff notes and preview, and a SHA-256 import
manifest are retained in [`assets/weather-icons`](assets/weather-icons/README.md).
The fourteen
weather illustrations keep integrated vector faces: happy/surprised sun,
gentle/frowning partly cloudy, neutral/frowning cloud and light rain, and
crying/angry heavy rain (normal/windy pairs). Closed-eye frowns have no eyebrows.
Partly cloudy shows only the cloud face, with the sun tucked behind it. Every
foreground cloud shares the same path, size, position and higher-contrast
cool light-gray gradient; only fully cloudy adds a darker rear cloud. Wind and
celestial details fit around the fixed cloud without squashing it.
Light rain has two strokes and heavy rain has four; wind-driven rain
reverses to down-right at a stronger slant, with visibly open wind curls.
Moon and unavailable icons use the same modern gradient finish.
Do not tint the icon ImageViews or replace these exports with approximate native
vector conversions. The native icon test checks exact packaged pixels against
the supplied PNGs and captures all sixteen icons at normal and compact sizes.

Text remains native and visually size-limited to avoid clipping at large font
settings; TalkBack retains complete intervals, units, provenance, freshness,
sunset and attribution. Tapping opens the forecast with visible source/license
information. Palette and layout details live in the root `DESIGN.md`.

The widget bundles Google Sans Regular and Bold v14.000 (the approved
open-source alternative to Product Sans), with license and provenance notices
in `app/src/main/assets/licenses/`. The 8dp rounded root marks itself as the
widget background so AOSP/Pixel Launcher respects the smaller radius without
adding inset cards; other launchers may still impose their own rounding.

## Widget data and refresh behavior

The decoder is a closed, depth-bounded pure-Kotlin implementation of the shared
schema. It validates strict UTF-8/JSON, the exact site/calendar/hour grid,
causal source clocks, mode/source pairings, finite field bounds, payload size
and aggregate status before cache replacement. The six shared v1 fixtures retain
calendar, provenance, freshness, expiry and complete-hour-coverage checks.
Android-specific tests cover the redesigned grouping, temperature selection,
condition icons and sunset geometry. The v1 API and paused iOS client are unchanged.

`AtomicFile` stores a bounded last-good snapshot and sanitized attempt metadata
in separate files under serialized access. Failed attempts never replace
weather. Missing/corrupt/impossible attempt metadata is conservative stale;
older callbacks cannot replace newer acquisitions. Successful attempt metadata
binds to the exact cached snapshot SHA-256, so interrupted or conflicting writes
render stale instead of inheriting an unrelated success. Each widget
independently persists Fahrenheit (default) or Celsius.

WorkManager coalesces one immediate network refresh and one network-constrained
30-minute periodic request. Both use the same process fetch lock and a short
attempt coalescing window. A separate unconstrained unique one-time worker
repaints at the earliest hour, correction, source-stale, acquisition-stale,
cutoff, midnight or 7am/hard-expiry boundary. Provider update, resize, boot, package,
worker, tap and configuration entrypoints recompute with the current clock.
These requests are best effort; Android does not guarantee exact execution.
All work is cancelled after the final widget is removed.

The fixed endpoint is:

```text
https://weather.ballydidean.farm/api/v3/sites/ballydidean/widget-forecast
```

The client follows no redirects, shares no cookies, requires JSON, reads at
most 128 KiB and applies one cancellable eight-second total deadline across
DNS/connect/headers/body. Errors persist only a bounded outcome category.

## Build and verification

The opt-in **Android internal release** GitHub workflow builds a signed AAB and
publishes to Google Play internal testing once Weather's upload-key and Play
service-account secrets are configured. See
[`docs/operations/android-release.md`](../../docs/operations/android-release.md)
for setup, explicit version selection, and the signed-artifact gates. Ordinary
local builds and the regular Check workflow remain credential-free and unsigned.

```bash
source mobile/android/scripts/android-env.sh
mobile/android/gradlew -p mobile/android --no-daemon \
  testDebugUnitTest lintDebug assembleDebug lintRelease assembleRelease
mobile/android/scripts/verify-release-artifact.sh
java mobile/android/scripts/GenerateBrandAssets.java --check
```

The Release APK is intentionally unsigned:

```text
mobile/android/app/build/outputs/apk/release/app-release-unsigned.apk
```

The artifact scan rejects debug fixtures/hosts, local origins, bridge or TLS
bypass markers, generated fixture certificate subjects/resources, fixture
runner arguments, credential-shaped values, protected fixture permissions and
cleartext-enabled manifests. It requires the canonical origin and fixed widget
endpoint. Passing proves unsigned Release isolation, not store readiness.

Run the 24-test Android 36 managed-device suite through the loopback-only HTTPS
fixture. The wrapper creates ephemeral certificates, exposes only its public
test CA, and removes every private key after the child exits:

```bash
mobile/scripts/with-native-https-fixture.sh \
  --evidence-dir /tmp/weather-android-https-fixture -- \
  mobile/android/scripts/run-hosted-shell-tests.sh \
  /tmp/weather-android-webview
```

This binds the real provider into a real `AppWidgetHost` at 276×102dp and
554×51dp, exercises the fixture matrix and persistent cache, and drives the
production WebView through real HTTPS login, settings, logout, history, popup,
origin-policy and certificate-negative journeys. Managed-device results are
under
`app/build/outputs/androidTest-results/managedDevice/debug/widgetPhone/` and
`app/build/reports/androidTests/managedDevice/`; the wrapper copies the exact
`TEST-widgetPhone.xml` receipt into the selected WebView evidence directory.

An already-running API 36 emulator can additionally prove WebView state across
real application process stops. This three-phase test logs in through the page,
stops the application externally, verifies the persisted HttpOnly session and
settings, logs out, stops it again, and verifies logout remains cleared:

```bash
mobile/scripts/with-native-https-fixture.sh \
  --evidence-dir /tmp/weather-android-process-https -- \
  mobile/android/scripts/run-hosted-shell-process-tests.sh \
  /tmp/weather-android-process emulator-5554
```

The device serial is explicit; the script never starts, stops or broadly kills
an emulator or the external fixture process.

For an already-running full emulator, the separate smoke exercises the ordinary
launcher placement flow and saves ignored evidence under
`mobile/android/host-evidence/`:

```bash
mobile/android/scripts/capture-host-evidence.sh
```

A custom host does not substitute for the launcher smoke, and neither path is
physical-device/store-signing evidence.

## Layout and attribution

The single-row RemoteViews layout preserves every pre-8pm interval, including
fall-back days, by increasing forecast block duration until the available
panels fit. Host tests cover 276×102dp portrait and 554×51dp landscape content
allocations, with platform host padding added outside those bounds, capped visual
text sizes and complete TalkBack descriptions. Missing
condition inputs use an unavailable icon, never invented fair weather.
`Open-Meteo · CC BY 4.0` remains accessible in widget descriptions and visible on
the forecast page opened by tapping the widget.

Launcher mipmaps are reproducible derivatives of the existing repository
master. Source provenance and exact `--write`/`--check` commands are in
`mobile/android/assets/README.md`.
