# Android hosted OTA updates

Weather's existing Android app loads the public HTTPS dashboard in its native
WebView. Web releases therefore do not need a new APK or a separate OTA
publisher. Deploy a validated immutable dashboard release to Blueberry using
the normal documented release process. That same release supplies the Android
web UI and the byte-distinct `/service-worker.js`.

## User experience

- Check once after startup and on foreground, coalescing checks within one
  minute. There is no background polling.
- Download a discovered release automatically and show an accessible progress
  bar and percentage based on verified shell assets.
- Show a persistent **Reload to apply** action only for a completely installed,
  waiting release. Leaving the prompt untouched defers the reload.
- On a known download failure, retain the usable current release and show
  **Retry download**. An unavailable update check alone does not show an error.
- Apply the exact waiting release, disable duplicate taps, and reload once when
  the new worker takes control. An activation timeout leaves a retryable action.
- Preserve browser cookies, units, adjustment settings, and other local
  preferences. The prompt survives dashboard rerenders and route changes.

Like Ferry FYI, the OTA prompt is not dismissible. Unlike Ferry's bundled
Capacitor app, Weather uses its existing hosted service worker rather than
Capgo. Existing installed APKs are identified by the default Android WebView
user agent; no native bridge, added permission, or app-store release is needed.
Ordinary browser clients retain automatic update activation without a native
prompt. A custom WebView user-agent override could disable native recognition.

## Release and safety boundary

The web server embeds SHA-256 subresource-integrity values for every
release-versioned JavaScript module and stylesheet into the worker. Installation
uses credential-free, cache-reloading same-origin requests, verifies those
bytes, rejects unsuccessful responses and HTML masquerading as JavaScript, and
requires every dashboard document's release metadata to match the worker.
Private or no-store dashboard responses cannot enter the shared shell.

Only after every verified response has been written is a final readiness marker
stored. The marker lets an idle waiting worker recover readiness after its
process restarts. Any failed staging writes are discarded without retiring the
old active cache. No partial download may activate. The old release's cache is
retired only after successful activation; unrelated caches remain untouched.

TLS, the controlled release pipeline, and the worker-embedded integrity map are
the authenticity boundary. This is not a detached-signature system, native
binary updater, or Capgo-style runtime health rollback. Cache storage remains
best-effort and may be evicted by Android or the user. Closing all old controlled
WebViews can naturally activate a waiting worker without tapping Reload.

The initial rollout into an app running the older update client may require
reopening the hosted page once to load the new updater. Later releases use the
staged prompt. Native widgets, permissions, and APK code still require the
[Android internal-release workflow](android-release.md).

Weather and map API data remain network-only. An offline reload can boot the
complete shell and preferences, but does not promise persisted forecast data.
Authenticated/admin pages remain outside the public offline cache.

## Verification

Compile the web workspace once, then run:

```bash
node --test apps/web/test/hosted-updates.test.mjs apps/web/test/service-worker.test.mjs
node --test apps/web/test/hosted-updates.e2e.mjs
```

The real-browser suite performs an Android-WebView-UA release transition,
checksum rejection, retry, visible progress, explicit apply, preference
retention, automatic desktop application, and offline reload with the ordinary
HTTP cache disabled. The production web-server contract binds its manifest to
the actual served executable bytes. After deployment, verify the live worker,
digest map, complete cache, and Android prompt layout on
<https://weather.ballydidean.farm/forecast>.

References: [service-worker lifecycle](https://web.dev/articles/service-worker-lifecycle),
[Android service workers](https://developer.android.com/reference/android/webkit/ServiceWorkerController),
[subresource integrity](https://developer.mozilla.org/en-US/docs/Web/Security/Subresource_Integrity).
