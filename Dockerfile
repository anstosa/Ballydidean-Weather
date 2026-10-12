# syntax=docker/dockerfile:1

ARG NODE_IMAGE=node:24.16.0-bookworm-slim

FROM ${NODE_IMAGE} AS dependencies
WORKDIR /opt/weather

COPY package.json package-lock.json ./
COPY apps/api/package.json apps/api/package.json
COPY apps/web/package.json apps/web/package.json
COPY apps/worker/package.json apps/worker/package.json
COPY packages/database/package.json packages/database/package.json
COPY packages/domain/package.json packages/domain/package.json
COPY packages/forecast-adjustment/package.json packages/forecast-adjustment/package.json
COPY packages/providers/package.json packages/providers/package.json
RUN npm ci --ignore-scripts

FROM dependencies AS build
COPY tsconfig.base.json ./
COPY apps apps
COPY packages packages
# compile the dependency graph once for both runtime targets
COPY scripts/build-workspaces.mjs scripts/build-workspaces.mjs
RUN npm run build

# export the cached build bytes for independent runtime image inspection
FROM scratch AS image-check-files
COPY --from=build /opt/weather/packages/forecast-adjustment/dist /forecast-adjustment/dist
COPY --from=build /opt/weather/packages/forecast-adjustment/package.json /forecast-adjustment/package.json
COPY --from=build /opt/weather/apps/web/dist/adjustment-maintenance-contract.mjs /web/adjustment-maintenance-contract.mjs

FROM dependencies AS production-dependencies
RUN npm prune --omit=dev --ignore-scripts

FROM ${NODE_IMAGE} AS runtime
ENV NODE_ENV=production
WORKDIR /opt/weather
RUN groupadd --gid 10002 weather \
  && useradd --uid 10002 --gid 10002 --no-create-home --shell /usr/sbin/nologin weather
COPY --from=production-dependencies /opt/weather/node_modules node_modules
COPY --from=production-dependencies /opt/weather/package.json package.json

FROM runtime AS server
COPY --from=build --chown=10002:10002 /opt/weather/apps/api/dist apps/api/dist
COPY --from=build --chown=10002:10002 /opt/weather/apps/worker/dist apps/worker/dist
COPY --from=build --chown=10002:10002 /opt/weather/packages/database/dist packages/database/dist
COPY --from=build --chown=10002:10002 /opt/weather/packages/domain/dist packages/domain/dist
COPY --from=build --chown=10002:10002 /opt/weather/packages/forecast-adjustment/dist packages/forecast-adjustment/dist
COPY --from=build --chown=10002:10002 /opt/weather/packages/providers/dist packages/providers/dist
COPY --chown=10002:10002 apps/api/package.json apps/api/package.json
COPY --chown=10002:10002 apps/worker/package.json apps/worker/package.json
COPY --chown=10002:10002 packages/database/package.json packages/database/package.json
COPY --chown=10002:10002 packages/domain/package.json packages/domain/package.json
COPY --chown=10002:10002 packages/forecast-adjustment/package.json packages/forecast-adjustment/package.json
COPY --chown=10002:10002 packages/providers/package.json packages/providers/package.json
COPY --chown=10002:10002 packages/database/migrations packages/database/migrations
COPY --chown=10002:10002 config/ecowitt config/ecowitt
COPY --chown=10002:10002 config/forecast-adjustments config/forecast-adjustments
COPY --chown=10002:10002 config/public-stations config/public-stations
COPY --chown=10002:10002 config/runtime-targets.json config/runtime-targets.json
COPY --chown=10002:10002 config/sites config/sites
COPY --chown=10002:10002 config/tempest config/tempest
COPY --chown=10002:10002 config/tides config/tides
COPY --chown=10002:10002 deploy/scripts/*.mjs deploy/scripts/
USER 10002:10002
CMD ["node", "apps/api/dist/main.js"]

FROM runtime AS web
RUN unlink node_modules/@weather/forecast-adjustment
COPY --from=build --chown=10002:10002 /opt/weather/apps/web/dist apps/web/dist
COPY --chown=10002:10002 apps/web/package.json apps/web/package.json
COPY --chown=10002:10002 apps/web/public apps/web/public
COPY --chown=10002:10002 config/sites/ballydidean.json config/sites/ballydidean.json
COPY --chown=10002:10002 deploy/scripts/web-server.mjs deploy/scripts/forecast-observations.mjs deploy/scripts/weather-admin-store.mjs deploy/scripts/home-network.mjs deploy/scripts/xweather-tile-cache.mjs deploy/scripts/xweather-usage-budget.mjs deploy/scripts/adjustment-evidence-store.mjs deploy/scripts/forecast-adjustment-scorecard-contract.mjs deploy/scripts/
# retain the exact source identity without invalidating compilation layers
ARG WEATHER_WEB_REVISION=development
ENV WEATHER_WEB_REVISION=${WEATHER_WEB_REVISION}
LABEL org.opencontainers.image.revision=${WEATHER_WEB_REVISION}
USER 10002:10002
CMD ["node", "deploy/scripts/web-server.mjs"]
