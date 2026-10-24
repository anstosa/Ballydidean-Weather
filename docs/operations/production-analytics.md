# Production analytics

The public production site loads Google tag `G-NYT2EZS8BX` only when the web
server runs with `NODE_ENV=production`, has a dated release, and the browser is
on `https://weather.ballydidean.farm`. Development and preview origins,
administrator login pages, and authenticated administrator shells do not load
the tag. Google signals and advertising personalization signals are disabled.
Analytics failures do not prevent the weather dashboard from loading.

Page views use the Google tag's automatic collection, without additional manual
SPA events. Keep **Enhanced measurement → Page views → Page changes based on
browser history events** enabled for this GA4 web stream, as described in
[Google's SPA guidance](https://developers.google.com/analytics/devguides/collection/ga4/single-page-applications).
Verify collection in the property's Realtime report after deployment; browser
network checks alone cannot verify access to or reporting inside the property.

The [web edge](../../deploy/scripts/web-server.mjs) owns tag injection.
[Browser acceptance tests](../../apps/web/test/web.e2e.mjs) cover production-only
collection and navigation; [deployment tests](../../deploy/test/deployment-contract.test.mjs)
also cover administrator and development-runtime exclusions.
