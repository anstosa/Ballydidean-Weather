# Web app installation

The public dashboard remains installable from iOS Safari and Android browser
menus through its standalone manifest, launcher icons, Apple touch icon and
verified offline application shell. Do not set `prefer_related_applications`:
the custom Android prompt links to Google Play without removing browser web
app installation.

On the homepage, mobile browser visitors see a dismissible prompt above the
navigation. iOS shows manual Safari Share → Add to Home Screen instructions,
including the Open as Web App option when present. Android links to the Weather
Play listing and its testing opt-in page. Play distribution is currently
**internal testing only**; users need an invited Google account. The website
cannot grant tester access or publish a production Play release.

Dismissal is stored on this browser/device, independently of measurement units
and adjustment preferences. Settings retains a non-dismissable installation
banner at the top, even after the homepage prompt is dismissed. If storage is
blocked, dismissal lasts only for the current document session.

The native Android WebView and standalone Home Screen launches show neither
surface. A normal browser cannot reliably discover an app installed elsewhere
on the device; the manual dismissal remains available in that case. No native
bridge, additional permission or dependency is introduced.

References: [Apple installation instructions](https://support.apple.com/guide/iphone/open-as-web-app-iphea86e5236/ios),
[Chrome web app installation criteria](https://web.dev/articles/install-criteria/),
[Google Play linking](https://developer.android.com/distribute/marketing-tools/linking-to-google-play).
