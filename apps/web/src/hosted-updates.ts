interface HostedUpdateState {
  readonly type: "WEATHER_UPDATE_STATE";
  readonly release: string;
  readonly phase: "downloading" | "ready" | "failed";
  readonly completed: number;
  readonly total: number;
}

// recognize existing android hosted shells without adding a native bridge
export function isAndroidHostedShell(userAgent: string): boolean {
  return /Android/u.test(userAgent) && /; wv\)/u.test(userAgent);
}

// accept only bounded worker progress metadata
export function parseHostedUpdateState(value: unknown): HostedUpdateState | null {
  // reject primitives and absent messages
  if (value === null || typeof value !== "object") {
    return null;
  }

  const state = value as Record<string, unknown>;
  // keep release labels and progress safe for rendering
  if (state.type !== "WEATHER_UPDATE_STATE" ||
    typeof state.release !== "string" || !/^[A-Za-z0-9][A-Za-z0-9.-]{0,63}$/u.test(state.release) ||
    !["downloading", "ready", "failed"].includes(state.phase as string) ||
    !Number.isInteger(state.completed) || !Number.isInteger(state.total) ||
    (state.total as number) < 1 || (state.total as number) > 256 ||
    (state.completed as number) < 0 || (state.completed as number) > (state.total as number) ||
    (state.phase === "ready" && state.completed !== state.total)) {
    return null;
  }

  return state as unknown as HostedUpdateState;
}

// stage complete hosted releases before asking android users to reload
export async function registerHostedUpdates(): Promise<void> {
  // leave unsupported browsers on the ordinary hosted path
  if (!("serviceWorker" in navigator)) {
    return;
  }

  const container = navigator.serviceWorker;
  const release = document.querySelector<HTMLMetaElement>('meta[name="weather-release"]')?.content ?? "development";
  const native = isAndroidHostedShell(navigator.userAgent);
  const controlled = container.controller !== null;
  const observed = new Set<ServiceWorker>();
  let registration: ServiceWorkerRegistration | null = null;
  let candidate: ServiceWorker | null = null;
  let state: HostedUpdateState | null = null;
  let prompt: HTMLElement | null = null;
  let applying = false;
  let reloading = false;
  let checking = false;
  let lastCheck = 0;
  let applyTimer: ReturnType<typeof setTimeout> | null = null;

  // replace only the independent update prompt across dashboard rerenders
  function render(message?: string): void {
    prompt?.remove();
    prompt = null;
    // keep browsers and current releases free of native update prompts
    if (!native || state === null || state.release === release) {
      return;
    }

    prompt = document.createElement("aside");
    prompt.className = "hosted-update-prompt";
    prompt.setAttribute("aria-label", "App update");
    const status = document.createElement("p");
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    status.textContent = message ?? (state.phase === "downloading"
      ? "Downloading app update…"
      : state.phase === "failed" ? "The app update could not be downloaded. Your current version is still available."
      : applying ? "Applying app update…" : "An app update is ready.");
    prompt.append(status);

    // show truthful asset-count progress while staging
    if (state.phase === "downloading") {
      const progress = document.createElement("progress");
      progress.max = state.total;
      progress.value = state.completed;
      progress.setAttribute("aria-label", "App update download");
      const percent = document.createElement("span");
      percent.textContent = `${Math.floor(state.completed / state.total * 100)}%`;
      prompt.append(progress, percent);
    } else {
      const action = document.createElement("button");
      action.type = "button";
      action.textContent = state.phase === "failed" ? "Retry download" : "Reload to apply";
      action.disabled = applying;
      // retry a known download or apply only the staged release
      action.addEventListener("click", () => {
        // keep failed downloads separate from activation
        if (state?.phase === "failed") {
          void check(true);
        } else {
          apply();
        }
      });
      prompt.append(action);
    }
    document.body.append(prompt);
  }

  // activate only the complete waiting worker selected by this page
  function apply(): void {
    // ignore duplicate taps and obsolete worker references
    if (applying || state?.phase !== "ready" || candidate === null || registration?.waiting !== candidate) {
      return;
    }

    applying = true;
    render();
    try {
      candidate.postMessage({ type: "WEATHER_APPLY_UPDATE", release: state.release });
      // retain a retryable action when activation does not complete
      applyTimer = setTimeout(() => {
        applying = false;
        render("The update could not be applied. Please try reloading again.");
      }, 15_000);
    } catch {
      applying = false;
      render("The update could not be applied. Please try reloading again.");
    }
  }

  // request verified metadata rather than trusting the page's old release label
  function observe(worker: ServiceWorker | null): void {
    // attach each worker only once
    if (worker === null || observed.has(worker)) {
      return;
    }

    observed.add(worker);
    candidate = worker;
    worker.postMessage({ type: "WEATHER_UPDATE_STATUS" });
    // query the completed install even when its broadcasts were missed
    worker.addEventListener("statechange", () => {
      // use only installed workers for activation
      if (worker.state === "installed") {
        worker.postMessage({ type: "WEATHER_UPDATE_STATUS" });
      }
    });
  }

  // reload once after a replacement takes ownership
  container.addEventListener("controllerchange", () => {
    // avoid first-install reloads and duplicate ownership events
    if (reloading || (!controlled && !applying)) {
      return;
    }

    reloading = true;
    // release the activation watchdog before document replacement
    if (applyTimer !== null) {
      clearTimeout(applyTimer);
    }
    window.location.reload();
  });

  // accept progress only from workers installed through this registration
  container.addEventListener("message", (event) => {
    const next = parseHostedUpdateState(event.data);
    const worker = event.source as ServiceWorker | null;
    // ignore unrelated messages and delayed activation broadcasts
    if (next === null || worker === null || !observed.has(worker) || worker !== candidate || reloading) {
      return;
    }
    // lifecycle readiness requires a fully installed waiting worker
    if (next.phase === "ready" && registration?.waiting !== worker) {
      return;
    }

    state = next;
    candidate = worker;
    render();
    // preserve browser auto-updates and silently align an already-current document
    if (next.phase === "ready" && (!native || next.release === release)) {
      apply();
    }
  });

  // check on startup foreground and explicit retry without background polling
  async function check(force = false): Promise<void> {
    // coalesce checks and rate-limit repeated foreground events
    if (registration === null || checking || (!force && Date.now() - lastCheck < 60_000)) {
      return;
    }

    checking = true;
    lastCheck = Date.now();
    try {
      await registration.update();
      observe(registration.installing ?? registration.waiting);
    } catch {
      // surface only failures after a newer release was actually found
      if (state !== null && state.phase !== "ready") {
        state = { ...state, phase: "failed" };
        render();
      }
    } finally {
      checking = false;
    }
  }

  registration = await container.register(
    "/service-worker.js",
    { scope: "/", updateViaCache: "none" },
  );
  // observe future installs before their first progress message
  registration.addEventListener("updatefound", () => observe(registration?.installing ?? null));
  observe(registration.installing ?? registration.waiting);
  // check again when the hosted app returns to the foreground
  document.addEventListener("visibilitychange", () => {
    // avoid downloading for hidden documents
    if (document.visibilityState === "visible") {
      void check();
    }
  });
  await check();
}
