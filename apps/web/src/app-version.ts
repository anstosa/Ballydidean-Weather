import { isAndroidHostedShell } from "./hosted-updates.js";

interface VersionEntry {
  readonly label: string;
  readonly value: string;
}

// report the loaded web revision and the installed native build independently
export function applicationVersions(userAgent: string, revision: string, release: string): readonly VersionEntry[] {
  const shortRevision = /^[a-f0-9]{40}$/u.test(revision) ? revision.slice(0, 7) : "Unavailable";
  // ordinary browsers and home-screen web apps identify their loaded source commit
  if (!isAndroidHostedShell(userAgent)) {
    return [{ label: "Version", value: revision === "development" ? "Development" : shortRevision }];
  }
  const name = userAgent.match(/(?:^|\s)BallydideanWeather\/([^\s]+)(?=\s|$)/u)?.[1] ?? "";
  const code = userAgent.match(/(?:^|\s)BallydideanWeatherBuild\/(\d{1,10})(?=\s|$)/u)?.[1] ?? "";
  const validName = name.length <= 64 && /^\d+\.\d+(?:\.\d+)?(?:-[A-Za-z0-9][A-Za-z0-9.-]*)?$/u.test(name);
  const validCode = Number(code) >= 1 && Number(code) <= 2_100_000_000;
  const appVersion = validName ? `${name}${validCode ? ` (build ${code})` : ""}` : "Not reported by this app build";
  const otaRelease = /^[A-Za-z0-9][A-Za-z0-9.-]{0,63}$/u.test(release) ? release : "Unavailable";
  return [
    { label: "App version", value: appVersion },
    { label: "OTA version", value: shortRevision === "Unavailable" ? otaRelease : `${otaRelease} (${shortRevision})` },
  ];
}

// fill the settings footer from this document rather than the latest server or waiting update
export function renderApplicationVersions(root: HTMLElement): void {
  const footer = root.querySelector<HTMLElement>("[data-app-version]");
  // keep other routes unchanged
  if (footer === null) {
    return;
  }
  const entries = applicationVersions(
    navigator.userAgent,
    document.querySelector<HTMLMetaElement>('meta[name="weather-revision"]')?.content ?? "",
    document.querySelector<HTMLMetaElement>('meta[name="weather-release"]')?.content ?? "",
  );
  const list = document.createElement("dl");
  // keep even native metadata on a text-only rendering boundary
  for (const entry of entries) {
    const term = document.createElement("dt");
    const value = document.createElement("dd");
    term.textContent = entry.label;
    value.textContent = entry.value;
    list.append(term, value);
  }
  footer.replaceChildren(list);
}
