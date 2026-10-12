import { isAndroidHostedShell } from "./hosted-updates.js";
import { browserUnitPreferenceStorage } from "./units.js";

export const APP_INSTALL_DISMISSED_KEY = "ballydidean-weather-install-dismissed-v1";
export const ANDROID_STORE_URL = "https://play.google.com/store/apps/details?id=farm.ballydidean.weather";
export const ANDROID_TESTING_URL = "https://play.google.com/apps/testing/farm.ballydidean.weather";
type InstallPlatform = "ios" | "android";

// offer installation only outside installed mobile shells
export function appInstallPlatform(
  userAgent: string,
  platform: string,
  maxTouchPoints: number,
  standalone: boolean,
): InstallPlatform | null {
  // keep installed apps and webviews free of installation prompts
  if (standalone || isAndroidHostedShell(userAgent)) {
    return null;
  }
  // include ipads using their desktop safari identity
  if (/iPhone|iPad|iPod/u.test(userAgent) || (platform === "MacIntel" && maxTouchPoints > 1)) {
    return "ios";
  }
  return /Android/u.test(userAgent) ? "android" : null;
}

// render only trusted platform-specific instructions and destinations
function installContent(platform: InstallPlatform, userAgent: string): string {
  // direct android visitors to the existing native application
  if (platform === "android") {
    return `<div class="app-install-copy"><strong>Get the Weather app</strong><p>Quick access and home-screen widgets.</p></div>
      <a class="app-install-action" href="${ANDROID_TESTING_URL}" target="_blank" rel="noopener">Install from Google Play</a>
      <p class="app-install-testing">Invited testers only. Join with your invited Google account. Already joined? <a href="${ANDROID_STORE_URL}" target="_blank" rel="noopener">Open the Play Store</a>.</p>`;
  }
  const safari = /Version\/.*Safari\//u.test(userAgent) && !/CriOS|FxiOS|EdgiOS/u.test(userAgent);
  return `<div class="app-install-copy"><strong>Add Weather to your Home Screen</strong><p>Open Weather like an app, without browser tabs.</p></div>
    <details class="app-install-instructions"><summary class="app-install-action">How to install</summary><ol>
      ${safari ? "" : "<li>Open this page in Safari.</li>"}
      <li>Open Safari’s Share menu (open the page menu first if needed).</li>
      <li>Choose <strong>Add to Home Screen</strong>.</li>
      <li>Turn on <strong>Open as Web App</strong> if shown, then tap <strong>Add</strong>.</li>
    </ol></details>`;
}

// retain the home toast outside weather rerenders and keep settings actionable
export function createAppInstallPrompt(root: HTMLElement): { update(view: string): void } {
  const displayMode = window.matchMedia("(display-mode: standalone)");
  const storage = browserUnitPreferenceStorage();
  let dismissed = false;
  let installed = false;
  let view = "";
  let toast: HTMLElement | null = null;
  let navigation: HTMLElement | null = null;

  try {
    dismissed = storage?.getItem(APP_INSTALL_DISMISSED_KEY) === "true";
  } catch {
    // retain a session dismissal when browser storage is unavailable
  }

  // place the toast above the actual navigation height, including safe areas
  function positionToast(): void {
    // reserve no content space on other pages or after dismissal
    if (toast === null || toast.hidden) {
      root.style.setProperty("--app-install-height", "0px");
      return;
    }
    const navTop = navigation !== null && getComputedStyle(navigation).position === "fixed"
      ? navigation.getBoundingClientRect().top : window.innerHeight;
    toast.style.bottom = `${Math.max(12, window.innerHeight - navTop + 12)}px`;
    root.style.setProperty("--app-install-height", `${toast.offsetHeight + 12}px`);
  }

  // measure after layout notifications so webkit never resizes observed ancestors mid-delivery
  const observer = new ResizeObserver(() => window.requestAnimationFrame(positionToast));

  // synchronize platform eligibility and local dismissal after each route render
  function update(nextView: string): void {
    view = nextView;
    const platform = appInstallPlatform(
      navigator.userAgent,
      navigator.platform,
      navigator.maxTouchPoints,
      installed || displayMode.matches || (navigator as Navigator & { standalone?: boolean }).standalone === true,
    );
    root.querySelector(".app-install-banner")?.remove();
    // remove installation surfaces inside installed or unsupported environments
    if (platform === null) {
      toast?.remove();
      toast = null;
      observer.disconnect();
      positionToast();
      return;
    }
    // build the dismissible prompt once so open instructions survive refreshes
    if (toast === null) {
      toast = document.createElement("aside");
      toast.className = "app-install-toast";
      toast.setAttribute("aria-label", "Install Weather");
      toast.innerHTML = `${installContent(platform, navigator.userAgent)}<button class="app-install-dismiss" type="button" aria-label="Dismiss installation prompt"><span aria-hidden="true">×</span></button>`;
      // remember the choice without removing the permanent settings destination
      toast.querySelector("button")?.addEventListener("click", () => {
        dismissed = true;
        try {
          storage?.setItem(APP_INSTALL_DISMISSED_KEY, "true");
        } catch {
          // respect the dismissal for this visit even with blocked storage
        }
        update(view);
        root.querySelector<HTMLElement>(".section-nav-home")?.focus({ preventScroll: true });
      });
      document.body.append(toast);
    }
    toast.hidden = view !== "home" || dismissed;
    // retain an undismissable installation destination at the top of settings
    if (view === "settings") {
      const banner = document.createElement("aside");
      banner.className = "app-install-banner";
      banner.setAttribute("aria-label", "Install Weather");
      banner.innerHTML = installContent(platform, navigator.userAgent);
      root.querySelector(".settings-page")?.prepend(banner);
    }
    observer.disconnect();
    navigation = root.querySelector<HTMLElement>(".section-nav");
    observer.observe(toast);
    // follow safe-area, text-size and orientation changes in the navigation
    if (navigation !== null) {
      observer.observe(navigation);
    }
    positionToast();
  }

  // hide prompts immediately when a browser installation completes
  window.addEventListener("appinstalled", () => {
    installed = true;
    update(view);
  });
  // hide prompts when an installed web app becomes the active display mode
  displayMode.addEventListener("change", () => update(view));
  // share dismissal across tabs without clearing other preferences
  window.addEventListener("storage", (event) => {
    // ignore unrelated preferences and storage areas
    if (event.storageArea === storage && event.key === APP_INSTALL_DISMISSED_KEY) {
      dismissed = event.newValue === "true";
      update(view);
    }
  });
  window.addEventListener("resize", positionToast);
  return { update };
}
