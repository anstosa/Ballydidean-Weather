# Android internal releases

The **Android internal release** workflow at
`.github/workflows/publish-android.yml` adapts Ferry FYI's Android publisher for
Weather. It builds `farm.ballydidean.weather` from `mobile/android`, retains a
signed APK and Android App Bundle for 30 days, and publishes only the AAB to
Google Play **internal testing**, with release status `completed`.

It does not publish to production, submit iOS builds, or deploy the website.
Weather is a native hosted shell, not Capacitor: no Ferry web build, Auth0,
Firebase, Sentry, Capgo, Yarn, or Ferry package identifiers are copied. Hosted
web UI releases use [service-worker OTA](android-ota-updates.md); native APK
and widget changes still use this Android release workflow.

## One-time setup

1. Create or confirm the Play Console app with package
   `farm.ballydidean.weather`, configure Play App Signing and internal testers,
   and finish the required app/content/privacy declarations. The privacy URL is
   <https://weather.ballydidean.farm/privacy>.
2. For an existing app, use its registered **upload key**, not a new key and not
   Ferry FYI's key. Keep the key and its backup outside this repository.
3. Enable the Google Play Android Developer API and give the service account
   access to this Weather app, including viewing app information and releasing
   to testing tracks. Keep permissions limited to the required app and tracks.
4. Complete the initial app/bundle setup in Play Console manually if this is a
   new app. The upload action cannot create a Play app record. A draft-only app
   also needs its Play setup completed before this workflow's `completed`
   internal release can succeed.
5. Add these **repository Actions secrets** in
   [Weather's Actions secret settings](https://github.com/anstosa/weather/settings/secrets/actions):

   | Secret | Value |
   | --- | --- |
   | `ANDROID_UPLOAD_KEYSTORE_BASE64` | Base64-encoded Weather upload keystore |
   | `ANDROID_UPLOAD_KEYSTORE_PASSWORD` | Keystore password |
   | `ANDROID_UPLOAD_KEY_ALIAS` | Registered upload key alias |
   | `ANDROID_UPLOAD_KEY_PASSWORD` | Private-key password |
   | `PLAY_SERVICE_ACCOUNT_JSON` | Entire service-account JSON key |

   No repository variables are required. Do not paste credentials into chat,
   workflow inputs, Git, or documentation. For example, stream the keystore
   directly into the secret instead of printing it:

   ```bash
   base64 -w 0 /private/path/weather-upload.jks |
     gh secret set ANDROID_UPLOAD_KEYSTORE_BASE64 --repo anstosa/weather
   gh secret set PLAY_SERVICE_ACCOUNT_JSON --repo anstosa/weather \
     < /private/path/play-service-account.json
   ```

   Use the Actions settings or `gh secret set NAME --repo anstosa/weather`
   interactively for the remaining values.

## Release an explicitly chosen version

Push the intended source branch and wait for its exact-commit **Check** to pass,
including selected native gates. Signing or uploading a different commit is
not authorized by another commit's successful check.

Choose the Android version name explicitly: `MAJOR.MINOR[.PATCH][-SUFFIX]`,
for example `0.1.0` or `0.1.0-beta.1`. Do not infer or silently bump a version
when releasing on someone else's behalf.

Once the workflow is present on the default branch, open
[Weather Actions](https://github.com/anstosa/weather/actions), choose **Android
internal release**, select the reviewed branch/tag, and enter that version.
Alternatively, after confirming the exact version and source, push an immutable
tag `android-vVERSION` pointing to the already-validated commit. Do not move or
reuse published tags. Date-based web release tags do not trigger this workflow.

To publish another internal build with the same explicitly chosen version name,
use a new immutable tag `android-vVERSION+build.YYYYMMDDTHHMMSSZ`, using the
current UTC timestamp for the build identity. For example,
`android-v0.1.0+build.20260924T173000Z` still publishes version name `0.1.0`;
the suffix distinguishes the source tag, not the app version. The workflow
generates a fresh numeric version code as usual. Never move the original
`android-v0.1.0` tag, and wait for the new exact-commit **Check** before pushing
the new build tag. This tag path also works before the workflow reaches `main`.

The workflow retains Ferry FYI's UTC `yyDDDHHmm` numeric version-code scheme,
not the caller's Actions run number. Releases are serialized, but this scheme
has minute precision: do not publish the same UTC minute twice. Existing apps
must have a highest Play version code below the newly generated value. These
checks do not query or modify Play's existing versions for you.

The workflow checks the Gradle wrapper and brand assets, builds unit tests and
Release lint, then builds both signed Release artifacts. It verifies Weather's
package, requested version, non-debuggable manifest, release isolation and
signatures before retaining or uploading them. The uploader is pinned to the
same `r0adkll/upload-google-play` v1.1.3 commit used by Ferry, with
`track: internal`; it is not Internal App Sharing.

Signing values are passed through environment variables. The decoded keystore
and Play service-account JSON are written under the runner's private temporary
directory, with mode `0600`, and an `always()` step removes both. The uploader
uses the private JSON path instead of creating a credential file in the checkout.
Release jobs disable Gradle's
configuration cache and never archive keys, credential properties, or Gradle
state. Ordinary builds without these environment variables remain unsigned.

## Local verification without publishing

```bash
node --test scripts/android-release.test.mjs
shellcheck -x mobile/android/scripts/android-version-code.sh \
  mobile/android/scripts/prepare-release.sh \
  mobile/android/scripts/verify-store-artifacts.sh
actionlint .github/workflows/publish-android.yml
```

An authorized signed build additionally sets the four `ANDROID_UPLOAD_*`
environment variables (`FILE`, `KEYSTORE_PASSWORD`, `KEY_ALIAS`, `KEY_PASSWORD`)
and explicit Gradle properties `VERSION_CODE`, `VERSION_NAME`, and
`weatherRequireReleaseSigning=true`. Use `--no-configuration-cache` when signing.
Never upload a locally generated test-key artifact to the real Play app.

Successful local verification does not establish that the GitHub secrets or
Play permissions are configured, that a release was uploaded, or that Google
has approved the app. Confirm the Actions result and Play internal-track release
after a separately authorized publishing run.

References: [Android app signing](https://developer.android.com/studio/publish/app-signing),
[Play service accounts](https://developers.google.com/android-publisher/getting_started),
[pinned upload action](https://github.com/r0adkll/upload-google-play/tree/935ef9c68bb393a8e6116b1575626a7f5be3a7fb).
