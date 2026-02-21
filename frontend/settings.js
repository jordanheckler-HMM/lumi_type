const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const state = {
  settings: null,
};

const els = {
  enabled: document.getElementById("enabled"),
  launchAtStartup: document.getElementById("launch_at_startup"),
  microphone: document.getElementById("microphone"),
  sensitivity: document.getElementById("sensitivity"),
  sensitivityValue: document.getElementById("sensitivity_value"),
  model: document.getElementById("model"),
  hotkey: document.getElementById("push_to_talk_hotkey"),
  save: document.getElementById("save"),
  requestPermissions: document.getElementById("request_permissions"),
  status: document.getElementById("status"),
  permissionsNotice: document.getElementById("permissions_notice"),
};

function setStatus(message) {
  els.status.textContent = message;
}

function currentFormSettings() {
  return {
    enabled: els.enabled.checked,
    launch_at_startup: els.launchAtStartup.checked,
    microphone: els.microphone.value,
    sensitivity: Number(els.sensitivity.value),
    model: els.model.value,
    push_to_talk_hotkey: els.hotkey.value.trim() || "Cmd+Shift+Space",
  };
}

function hydrateForm(settings) {
  state.settings = settings;
  els.enabled.checked = Boolean(settings.enabled);
  els.launchAtStartup.checked = Boolean(settings.launch_at_startup);
  els.sensitivity.value = settings.sensitivity ?? 0.45;
  els.sensitivityValue.value = Number(els.sensitivity.value).toFixed(2);
  els.model.value = settings.model;
  els.hotkey.value = settings.push_to_talk_hotkey;
}

async function loadMicrophones(selected) {
  const devices = await invoke("list_input_devices").catch(() => []);
  els.microphone.innerHTML = "";

  const defaultOption = document.createElement("option");
  defaultOption.value = "";
  defaultOption.textContent = "System Default";
  els.microphone.append(defaultOption);

  for (const name of devices) {
    const option = document.createElement("option");
    option.value = name;
    option.textContent = name;
    els.microphone.append(option);
  }

  els.microphone.value = selected || "";
}

async function saveSettings() {
  const next = currentFormSettings();
  await invoke("update_settings", { next });
  state.settings = next;
  setStatus("Settings saved");
}

async function requestPermissions() {
  const status = await invoke("request_permissions");
  const missing = !status.microphone || !status.accessibility;
  els.permissionsNotice.classList.toggle("hidden", !missing);

  if (missing) {
    setStatus("Grant both permissions in System Settings, then return to LumiType.");
  } else {
    setStatus("Permissions granted.");
  }
}

async function bootstrap() {
  const settings = await invoke("get_settings");
  hydrateForm(settings);
  await loadMicrophones(settings.microphone);

  els.save.addEventListener("click", async () => {
    try {
      await saveSettings();
    } catch (error) {
      setStatus(String(error));
    }
  });

  els.requestPermissions.addEventListener("click", async () => {
    try {
      await requestPermissions();
    } catch (error) {
      setStatus(String(error));
    }
  });

  els.sensitivity.addEventListener("input", () => {
    els.sensitivityValue.value = Number(els.sensitivity.value).toFixed(2);
  });

  listen("permissions-required", ({ payload }) => {
    const missing = !payload.microphone || !payload.accessibility;
    els.permissionsNotice.classList.toggle("hidden", !missing);
  });

  listen("engine-error", ({ payload }) => {
    setStatus(payload);
  });
}

bootstrap().catch((error) => setStatus(String(error)));
