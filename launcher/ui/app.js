// Launcher UI. Talks to the Rust side through Tauri commands (see src/main.rs).
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dialog = window.__TAURI__.dialog;
const opener = window.__TAURI__.opener;

const LANGS = [
  {
    id: "en", name: "English", variant: "Catbox English", by: "&Olga",
    repo: "https://github.com/andOlga/umineko-catbox-english",
    about: "The complete English translation of the Switch release.",
    credits: "Translation patch by &Olga and the contributors credited in its repository.",
  },
  {
    id: "enx", name: "English", variant: "Catbox X", by: "Foxdear",
    repo: "https://github.com/Foxdear/umineko-catbox-X-english",
    about: "A revision of Catbox English: currently revised up to Episode 7, fixes minor issues.",
    credits: "Revision by Foxdear, building on &Olga's Catbox English.",
  },
  {
    id: "ja", name: "Japanese", variant: "Original", by: "Entergram",
    about: "The original Japanese text. Comes from the 1.0.3 update in your own dump.",
    credits: "Text by Ryukishi07 / 07th Expansion; Switch release by Entergram.",
  },
];

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

let state = null;
let showSetup = false;
const picks = { base: null, update: null, prodKeys: null, titleKeys: null };
const downloads = {}; // id -> fraction while downloading
const latest = {}; // id -> latest release tag, after "check for updates"
const errors = {}; // id -> last download error

function say(el, text, kind = "") {
  el.textContent = text;
  el.className = `message ${kind}`;
}

async function refresh() {
  state = await invoke("get_state");
  render();
}

function selectedLanguage() {
  const installed = (id) => state.languages[id] != null;
  const remembered = state.settings.language;
  return installed(remembered) ? remembered : LANGS.map((l) => l.id).find(installed) ?? null;
}

function render() {
  const setupVisible = showSetup || !state.data_ready;
  $("#setup").hidden = !setupVisible;
  $("#main").hidden = setupVisible;
  $("#setup-data").textContent = state.data_dir;
  $("#setup-cancel").hidden = !state.data_ready;
  renderPicker();
  renderCards();
  renderSettings();
}

// --- Play view ---

function renderPicker() {
  const picker = $("#lang-picker");
  $$(".lang-option", picker).forEach((n) => n.remove());
  const selected = selectedLanguage();
  for (const lang of LANGS) {
    const installed = state.languages[lang.id] != null;
    const row = document.createElement("label");
    row.className = `lang-option${installed ? "" : " unavailable"}${lang.id === selected ? " checked" : ""}`;
    row.innerHTML = `<input type="radio" name="lang"><span class="lang-name"></span>`;
    const input = $("input", row);
    input.value = lang.id;
    input.disabled = !installed;
    input.checked = lang.id === selected;
    $(".lang-name", row).innerHTML = `${lang.name} <small></small>`;
    $(".lang-name small", row).textContent = `${lang.variant} · ${lang.by}`;
    if (!installed) row.append(installButton(lang, "small"));
    input.addEventListener("change", () => {
      state.settings.language = lang.id;
      invoke("save_settings", { settings: state.settings });
      renderPicker();
    });
    picker.append(row);
  }
  $("#play").disabled = !selected;
}

function installButton(lang, size = "") {
  const btn = document.createElement("button");
  btn.className = `btn ${size}`;
  if (lang.id === "ja") {
    btn.textContent = state.languages.ja ? "Set up again" : "Set up";
    btn.title = "Japanese needs the 1.0.3 update NSP from your dump";
    btn.addEventListener("click", (e) => { e.preventDefault(); showSetup = true; render(); });
    return btn;
  }
  const busy = lang.id in downloads;
  const installed = state.languages[lang.id];
  btn.textContent = busy ? "Downloading…" : !installed ? "Download" : latest[lang.id] && latest[lang.id] !== installed ? "Update" : "Reinstall";
  btn.disabled = busy;
  btn.dataset.dl = lang.id;
  if (!installed) btn.classList.add("primary");
  btn.addEventListener("click", (e) => { e.preventDefault(); install(lang); });
  return btn;
}

async function install(lang) {
  downloads[lang.id] = 0;
  delete errors[lang.id];
  render();
  try {
    await invoke("install_language", { id: lang.id });
  } catch (e) {
    errors[lang.id] = String(e);
  } finally {
    delete downloads[lang.id];
    await refresh();
  }
}

$("#play").addEventListener("click", async () => {
  const lang = selectedLanguage();
  const msg = $("#play-msg");
  try {
    $("#play").disabled = true;
    await invoke("launch", { lang });
    say(msg, "The game is starting…", "ok");
  } catch (e) {
    say(msg, String(e), "error");
  } finally {
    setTimeout(() => { $("#play").disabled = false; }, 3000);
  }
});

// --- Languages view ---

function renderCards() {
  const box = $("#lang-cards");
  box.replaceChildren();
  for (const lang of LANGS) {
    const card = document.createElement("article");
    card.className = "card";
    card.innerHTML = `<h4></h4><p class="about"></p><p class="credits"></p><div class="card-side"><div class="status"></div></div>`;
    $("h4", card).innerHTML = `${lang.variant} <small></small>`;
    $("h4 small", card).textContent = `${lang.name} · ${lang.by}`;
    $(".about", card).textContent = lang.about;
    $(".credits", card).textContent = lang.credits + " ";
    if (lang.repo) {
      const a = document.createElement("a");
      a.href = lang.repo;
      a.textContent = "Repository";
      $(".credits", card).append(a);
    }
    const installed = state.languages[lang.id];
    const status = $(".status", card);
    status.textContent = errors[lang.id] ?? (installed ? `Installed ${installed}` : "Not installed");
    status.classList.toggle("installed", !!installed && !errors[lang.id]);
    if (latest[lang.id] && installed && latest[lang.id] !== installed) status.textContent += ` · ${latest[lang.id]} available`;
    else if (latest[lang.id] && installed) status.textContent += " · up to date";
    const side = $(".card-side", card);
    side.append(installButton(lang, "small"));
    if (lang.id in downloads) {
      const bar = document.createElement("div");
      bar.className = "bar";
      bar.dataset.bar = lang.id;
      bar.innerHTML = `<span style="width:${downloads[lang.id] * 100}%"></span>`;
      side.append(bar);
    }
    box.append(card);
  }
}

$("#check-updates").addEventListener("click", async (e) => {
  e.target.disabled = true;
  e.target.textContent = "Checking…";
  const results = await invoke("check_updates");
  results.forEach((r, i) => {
    const id = LANGS[i].id;
    if (r.Ok) { latest[id] = r.Ok.tag; delete errors[id]; } else errors[id] = r.Err;
  });
  e.target.disabled = false;
  e.target.textContent = "Check for updates";
  renderCards();
  renderPicker();
});

// --- Settings view ---

function renderSettings() {
  const dataDir = $("#data-dir");
  dataDir.textContent = state.data_dir;
  dataDir.classList.add("set");
  $("#fullscreen").checked = state.settings.fullscreen;
  $("#debug-logging").checked = state.settings.debug_logging;
}

async function saveSettings() {
  await invoke("save_settings", { settings: state.settings });
  await refresh();
}

$("#fullscreen").addEventListener("change", (e) => { state.settings.fullscreen = e.target.checked; saveSettings(); });
$("#debug-logging").addEventListener("change", (e) => { state.settings.debug_logging = e.target.checked; saveSettings(); });

$("#move-data").addEventListener("click", async () => {
  const target = await dialog.open({ directory: true, title: "Choose an empty folder for the game data" });
  if (!target) return;
  const msg = $("#settings-msg");
  say(msg, "Moving the game data… (this can take a few minutes across drives)");
  try {
    await invoke("move_data_dir", { target });
    say(msg, "Moved.", "ok");
  } catch (e) {
    say(msg, String(e), "error");
  }
  await refresh();
});

$("#rerun-setup").addEventListener("click", () => { showSetup = true; render(); });

// --- Setup screen ---

const FILTERS = {
  base: [{ name: "Switch package (NSP)", extensions: ["nsp"] }],
  update: [{ name: "Switch package (NSP)", extensions: ["nsp"] }],
  prodKeys: [{ name: "Key file", extensions: ["keys", "txt"] }, { name: "All files", extensions: ["*"] }],
  titleKeys: [{ name: "Key file", extensions: ["keys", "txt"] }, { name: "All files", extensions: ["*"] }],
};

$$("[data-pick]").forEach((btn) => btn.addEventListener("click", async () => {
  const key = btn.dataset.pick;
  const path = await dialog.open({ filters: FILTERS[key] });
  if (!path) return;
  picks[key] = path;
  const out = $(`[data-path="${key}"]`);
  out.textContent = path;
  out.title = path;
  out.classList.add("set");
}));

$("#setup-cancel").addEventListener("click", () => { showSetup = false; render(); });

$("#setup-start").addEventListener("click", async () => {
  const msg = $("#setup-msg");
  if (!picks.base || !picks.prodKeys) {
    say(msg, "Please choose at least the base game NSP and prod.keys.", "error");
    return;
  }
  const controls = $$("#setup button");
  controls.forEach((b) => (b.disabled = true));
  $("#setup-progress").hidden = false;
  say(msg, "This takes a few minutes. Please keep the launcher open.");
  try {
    const warnings = await invoke("run_setup", { base: picks.base, update: picks.update, prodKeys: picks.prodKeys, titleKeys: picks.titleKeys });
    $("#setup-progress").hidden = true;
    say(msg, ["The game data is ready.", ...warnings].join("\n"), warnings.length ? "warn" : "ok");
    await refresh();
    setTimeout(() => { showSetup = false; render(); }, warnings.length ? 6000 : 1500);
  } catch (e) {
    $("#setup-progress").hidden = true;
    say(msg, String(e), "error");
  } finally {
    controls.forEach((b) => (b.disabled = false));
  }
});

// --- shared ---

listen("progress", ({ payload }) => {
  const percent = `${Math.round(payload.fraction * 100)}%`;
  if (payload.task === "setup") {
    $("#setup-progress .progress-label").textContent = `${payload.label}… ${percent}`;
    $("#setup-progress .bar span").style.width = percent;
    return;
  }
  downloads[payload.task] = payload.fraction;
  $$(`[data-bar="${payload.task}"] span`).forEach((bar) => (bar.style.width = percent));
  $$(`[data-dl="${payload.task}"]`).forEach((btn) => (btn.textContent = `Downloading ${percent}`));
});

$$(".nav button").forEach((btn) => btn.addEventListener("click", () => {
  $$(".nav button").forEach((b) => b.classList.toggle("active", b === btn));
  $$(".view").forEach((v) => (v.hidden = v.id !== `view-${btn.dataset.view}`));
}));

// links open in the browser, never inside the launcher window
document.addEventListener("click", (e) => {
  const a = e.target.closest("a[href^='http']");
  if (a) { e.preventDefault(); opener.openUrl(a.href); }
});

refresh();
