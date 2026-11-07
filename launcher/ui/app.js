// Launcher UI. Talks to the Rust side through Tauri commands (see src/main.rs).
"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dialog = window.__TAURI__.dialog;
const opener = window.__TAURI__.opener;
const updater = window.__TAURI__.updater;
const processApi = window.__TAURI__.process;

const LANGS = [
  {
    id: "en", name: "English", variant: "Catbox English", by: "&Olga",
    repo: "https://github.com/andOlga/umineko-catbox-english",
    about: "The full English translation.",
    credits: "By &Olga and contributors.",
  },
  {
    id: "enx", name: "English", variant: "Catbox X", by: "Foxdear",
    repo: "https://github.com/Foxdear/umineko-catbox-X-english",
    about: "Catbox English with fixes, up to Episode 7.",
    credits: "By Foxdear, based on Catbox English.",
  },
  {
    id: "ja", name: "Japanese", variant: "Original", by: "Entergram",
    about: "The original text. Needs the 1.0.3 update.",
    credits: "By Ryukishi07 / 07th Expansion and Entergram.",
  },
];

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

let state = null; // until it arrives (and while moving in from %APPDATA%), no screen shows: only the sky
let migrating = false;
let showSetup = false;
let showUninstall = false;
let installDir = null; // chosen on the install screen
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
  if (!state || migrating) return;
  const program = state.program;
  const screen = program.uninstall || showUninstall ? "uninstall"
    : program.can_install && !program.installed ? "install"
    : showSetup || !state.data_ready ? "setup" : "main";
  $$(".screen").forEach((s) => (s.hidden = s.id !== screen));
  if (screen !== "main") $("#notice").hidden = true;
  installDir ??= program.folder;
  $("#install-dir").textContent = installDir;
  $("#install-dir").title = installDir;
  $("#uninstall-dir").textContent = program.folder;
  $("#uninstall-data-dir").textContent = state.data_dir;
  $("#uninstall-saves").textContent = state.savedata.replace(/[\\/]SAVEDATA$/, "");
  $("#open-uninstall").hidden = !program.installed;
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
    setTimeout(() => say(msg, ""), 5000);
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
  say(msg, "Moving the game data…");
  try {
    await invoke("move_data_dir", { target });
    say(msg, "Moved.", "ok");
  } catch (e) {
    say(msg, String(e), "error");
  }
  await refresh();
});

$("#rerun-setup").addEventListener("click", () => { showSetup = true; render(); });

// --- Launcher updates ---

let pendingUpdate = null;

// manual: say "up to date" or the error; on start a failure (offline, no release yet) stays silent
async function checkAppUpdate(manual) {
  const msg = $("#app-update-msg");
  if (manual) say(msg, "Checking…");
  try {
    const update = await updater.check();
    if (!update) {
      if (manual) say(msg, "You have the latest version.", "ok");
      return;
    }
    pendingUpdate = update;
    $("#update-title").textContent = `Open Catbox ${update.version}`;
    // one bullet per line: the notes come hard-wrapped from the changelog
    const notes = await invoke("release_notes", { body: update.body ?? "" });
    $("#update-notes").textContent = notes.map((n) => `• ${plain(n)}`).join("\n");
    $("#update-banner").hidden = false;
    $("#update-now").hidden = false;
    if (manual) say(msg, `Version ${update.version} is available.`, "ok");
  } catch (e) {
    if (manual) say(msg, `Could not check for updates.\n${e}`, "error");
  }
}

$("#check-app-update").addEventListener("click", async (e) => {
  e.target.disabled = true;
  await checkAppUpdate(true);
  e.target.disabled = false;
});

$("#update-later").addEventListener("click", () => { $("#update-banner").hidden = true; });

$("#update-now").addEventListener("click", async () => {
  const buttons = $$("#update-banner button");
  buttons.forEach((b) => (b.disabled = true));
  const msg = $("#update-msg");
  $("#update-progress").hidden = false;
  say(msg, "Downloading…");
  try {
    // downloads and checks the new version, starts it and closes this launcher; the new one opens by itself
    await invoke("install_update");
  } catch (e) {
    say(msg, `The update failed: ${e}`, "error");
    buttons.forEach((b) => (b.disabled = false));
  }
});

// --- Installing and uninstalling the launcher ---

$("#install-browse").addEventListener("click", async () => {
  const picked = await dialog.open({ directory: true, title: "Choose where to install Open Catbox" });
  if (!picked) return;
  // always a folder of its own, so uninstalling never has to touch anything else
  installDir = /[\\/]Open Catbox$/i.test(picked) ? picked : `${picked.replace(/[\\/]+$/, "")}\\Open Catbox`;
  render();
});

$("#install-start").addEventListener("click", async () => {
  const controls = $$("#install button, #install input");
  controls.forEach((c) => (c.disabled = true));
  $("#install-progress").hidden = false;
  say($("#install-msg"), "");
  try {
    // on success the installed copy starts and this window closes
    await invoke("install_program", { folder: installDir, menu: $("#install-menu").checked, desktop: $("#install-desktop").checked });
  } catch (e) {
    $("#install-progress").hidden = true;
    say($("#install-msg"), String(e), "error");
    controls.forEach((c) => (c.disabled = false));
  }
});

$("#open-uninstall").addEventListener("click", () => { showUninstall = true; render(); });

$("#uninstall-cancel").addEventListener("click", () => {
  if (state.program.uninstall) processApi.exit(0); // opened from Installed apps just for this
  showUninstall = false;
  render();
});

$("#uninstall-start").addEventListener("click", async () => {
  const controls = $$("#uninstall button, #uninstall input");
  controls.forEach((c) => (c.disabled = true));
  const msg = $("#uninstall-msg");
  say(msg, "Uninstalling…");
  try {
    await invoke("uninstall_program", { deleteData: $("#uninstall-data").checked });
    say(msg, "Open Catbox was removed. Farewell.", "ok");
    setTimeout(() => processApi.exit(0), 2000);
  } catch (e) {
    say(msg, String(e), "error");
    controls.forEach((c) => (c.disabled = false));
  }
});

// --- Version history ---

// markdown emphasis and code marks, shown as plain text
const plain = (note) => note.replace(/\*\*?|`/g, "");

function renderHistory(history) {
  const list = $("#history-list");
  list.replaceChildren();
  for (const entry of history.entries) {
    const item = document.createElement("article");
    item.className = "release";
    const head = document.createElement("h4");
    head.textContent = `v${entry.version}`;
    if (entry.version === history.installed) {
      item.classList.add("current");
      const tag = document.createElement("span");
      tag.className = "tag";
      tag.textContent = "Installed";
      head.append(tag);
    }
    const date = document.createElement("small");
    date.textContent = entry.date;
    head.append(date);
    const ul = document.createElement("ul");
    for (const note of entry.notes) {
      const li = document.createElement("li");
      li.textContent = plain(note);
      ul.append(li);
    }
    item.append(head, ul);
    list.append(item);
  }
}

let historyLoaded = false;
async function loadHistory() {
  if (historyLoaded) return;
  historyLoaded = true;
  renderHistory(await invoke("version_history", { online: false }));
  const history = await invoke("version_history", { online: true });
  renderHistory(history);
  // offline only hides releases newer than this launcher, so a quiet note is enough
  if (history.online_error) say($("#history-msg"), "Could not check for newer releases.", "warn");
}

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
    say(msg, "Choose the base game NSP and prod.keys.", "error");
    return;
  }
  const controls = $$("#setup button");
  controls.forEach((b) => (b.disabled = true));
  $("#setup-progress").hidden = false;
  say(msg, "This takes a few minutes. Keep the launcher open.");
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

// a folder that already holds data.rom (another PC, a backup, an earlier install): copied here or used where it is
let existing = null;

$("#existing-pick").addEventListener("click", async () => {
  const folder = await dialog.open({ directory: true, title: "Choose the folder with data.rom" });
  if (folder) checkExisting(folder);
});

async function checkExisting(folder) {
  const msg = $("#setup-msg");
  const controls = $$("#setup button");
  existing = null;
  $("#existing-choice").hidden = true;
  $("#existing-path").textContent = folder;
  $("#existing-path").title = folder;
  $("#existing-path").classList.add("set");
  controls.forEach((b) => (b.disabled = true));
  $("#setup-progress").hidden = false;
  say(msg, "Checking data.rom…");
  try {
    const known = await invoke("check_game_folder", { folder });
    existing = folder;
    say(msg, known ? "Game data OK." : "Unknown data.rom version. It may still work.",
      known ? "ok" : "warn");
    $("#existing-choice").hidden = false;
  } catch (e) {
    say(msg, String(e), "error");
  } finally {
    $("#setup-progress").hidden = true;
    controls.forEach((b) => (b.disabled = false));
  }
}

async function useExisting(copy) {
  const msg = $("#setup-msg");
  const controls = $$("#setup button");
  controls.forEach((b) => (b.disabled = true));
  $("#setup-progress").hidden = !copy;
  say(msg, copy ? "Copying the game data…" : "");
  try {
    await invoke("use_game_folder", { folder: existing, copy });
    say(msg, "The game data is ready.", "ok");
    await refresh();
    setTimeout(() => { showSetup = false; render(); }, 1500);
  } catch (e) {
    say(msg, String(e), "error");
  } finally {
    $("#setup-progress").hidden = true;
    controls.forEach((b) => (b.disabled = false));
  }
}

$("#existing-copy").addEventListener("click", () => useExisting(true));
$("#existing-use").addEventListener("click", () => useExisting(false));

// --- Moving in from %APPDATA% (first start of a portable home) ---

async function moveIn() {
  migrating = true;
  let said;
  try {
    said = await invoke("migrate");
  } catch (e) {
    said = [`Some settings could not be brought over: ${e}`];
  }
  migrating = false;
  $("#migrate").hidden = true;
  await refresh();
  if (said.length) {
    $("#notice-text").textContent = [`Your settings are now in ${state.home}.`, ...said].join("\n");
    $("#notice").hidden = false;
  }
}

$("#notice-ok").addEventListener("click", () => { $("#notice").hidden = true; });

// --- shared ---

listen("progress", ({ payload }) => {
  const percent = `${Math.round(payload.fraction * 100)}%`;
  if (payload.task === "install") {
    $("#install-progress .progress-label").textContent = `${payload.label}…`;
    $("#install-progress .bar span").style.width = percent;
    return;
  }
  if (payload.task === "app-update") {
    $("#update-progress span").style.width = percent;
    say($("#update-msg"), payload.fraction >= 1 ? "Installing… the launcher restarts by itself." : `Downloading… ${percent}`);
    return;
  }
  if (payload.task === "migrate") {
    if (!migrating) return;
    // only a copy across drives reports progress; a rename is done before anything would show
    $$(".screen").forEach((s) => (s.hidden = s.id !== "migrate"));
    $("#migrate-progress .progress-label").textContent = `${payload.label}… ${percent}`;
    $("#migrate-progress .bar span").style.width = percent;
    return;
  }
  if (payload.task === "move") {
    say($("#settings-msg"), `${payload.label}… ${percent}`);
    return;
  }
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
  if (btn.dataset.view === "history") loadHistory();
}));

// links open in the browser, never inside the launcher window
document.addEventListener("click", (e) => {
  const a = e.target.closest("a[href^='http']");
  if (a) { e.preventDefault(); opener.openUrl(a.href); }
});

window.__TAURI__.app.getVersion().then((v) => $$(".app-version").forEach((n) => (n.textContent = `v${v}`)));
invoke("get_state").then(async (first) => {
  state = first;
  if (first.migration && !first.program.uninstall) await moveIn();
  else render();
  if (state.program.update_error) {
    $("#update-title").textContent = "The update did not finish";
    say($("#update-msg"), state.program.update_error, "error");
    $("#update-now").hidden = true;
    $("#update-banner").hidden = false;
  }
  if (!state.program.uninstall) checkAppUpdate(false);
});
