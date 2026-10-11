import {
  mountWeatherDashboard,
  type WeatherDashboardController,
  type WeatherView,
} from "./index.js";
import { registerHostedUpdates } from "./hosted-updates.js";

const root = document.querySelector<HTMLElement>("#weather-app");

// initialize one google tag only on the public production origin
function initializeProductionAnalytics(isAdmin: boolean): void {
  // keep development, previews and authenticated shells out of production analytics
  if (document.documentElement.dataset.weatherAnalytics !== "true" ||
    window.location.origin !== "https://weather.ballydidean.farm" ||
    isAdmin || document.getElementById("weather-google-tag") !== null) {
    return;
  }

  const analyticsWindow = window as Window & {
    dataLayer?: unknown[];
    gtag?: (...parameters: unknown[]) => void;
  };
  const dataLayer = analyticsWindow.dataLayer ??= [];
  // preserve the google tag's arguments-based queue before its async script loads
  analyticsWindow.gtag = function (..._parameters: unknown[]): void {
    dataLayer.push(arguments);
  };
  analyticsWindow.gtag("js", new Date());
  // retain automatic page views without duplicate manual history events
  analyticsWindow.gtag("config", "G-NYT2EZS8BX", {
    allow_google_signals: false,
    allow_ad_personalization_signals: false,
  });
  const script = document.createElement("script");
  script.id = "weather-google-tag";
  script.async = true;
  script.src = "https://www.googletagmanager.com/gtag/js?id=G-NYT2EZS8BX";
  document.head.append(script);
}

// resolve one supported browser route
function resolveView(pathname: string): WeatherView {
  // open the protected property sensor editor
  if (pathname === "/admin" || pathname === "/admin/") {
    return "admin";
  }

  // open the historical log
  if (pathname === "/logs" || pathname === "/logs/") {
    return "logs";
  }

  // open the station map
  if (pathname === "/map" || pathname === "/map/") {
    return "map";
  }

  // open the daily forecast
  if (pathname === "/forecast" || pathname === "/forecast/") {
    return "forecast";
  }

  // open historical trends
  if (pathname === "/trends" || pathname === "/trends/") {
    return "trends";
  }

  // open device display preferences
  if (pathname === "/settings" || pathname === "/settings/") {
    return "settings";
  }

  return "home";
}

// select the history surface visible to the user
function resolveHistoryWindow(): Window | null {
  const topWindow = window.top;

  // keep standalone navigation on the application window
  if (topWindow === null || topWindow === window) {
    return window;
  }

  try {
    // synchronize a same-origin embedding host
    if (topWindow.location.origin === window.location.origin) {
      return topWindow;
    }
  } catch {
    // preserve native navigation across isolated frames
    return null;
  }

  return null;
}

// connect public page links to browser history
function bindBrowserNavigation(
  root: HTMLElement,
  controller: WeatherDashboardController,
): void {
  const historyWindow = resolveHistoryWindow();

  // let cross-origin frames perform observable document navigation
  if (historyWindow === null) {
    return;
  }

  root.addEventListener("click", (event) => {
    // preserve modified clicks and previously handled events
    if (
      event.defaultPrevented ||
      event.button !== 0 ||
      event.metaKey ||
      event.ctrlKey ||
      event.shiftKey ||
      event.altKey
    ) {
      return;
    }

    const target = event.target instanceof Element ? event.target : null;
    const link = target?.closest<HTMLAnchorElement>("a[data-weather-route]") ?? null;

    // preserve unrelated and external links
    if (link === null) {
      return;
    }

    const destination = new URL(link.href, window.location.href);

    // retain native navigation across origins
    if (destination.origin !== window.location.origin) {
      return;
    }

    event.preventDefault();
    const nextLocation = `${destination.pathname}${destination.search}${destination.hash}`;

    // avoid duplicate history entries for the active route
    if (nextLocation === `${historyWindow.location.pathname}${historyWindow.location.search}${historyWindow.location.hash}`) {
      return;
    }

    historyWindow.history.pushState(null, "", nextLocation);
    void controller.setView(resolveView(destination.pathname));
  });

  historyWindow.addEventListener("popstate", () => {
    // restore content when browser history changes
    void controller.setView(resolveView(historyWindow.location.pathname));
  });
}

const view = resolveView(window.location.pathname);
// retain the authenticated shell boundary across SPA navigation
const isAdmin = document.documentElement.dataset.weatherAdmin === "true" || view === "admin";

// mount only when the application shell is present
if (root !== null) {
  const controller = mountWeatherDashboard(root, { isAdmin, view });
  bindBrowserNavigation(root, controller);
  try {
    initializeProductionAnalytics(isAdmin);
  } catch {
    // keep optional analytics from interrupting the weather application
  }
}

registerHostedUpdates().catch(
  // keep optional install support from interrupting weather rendering
  () => undefined,
);
