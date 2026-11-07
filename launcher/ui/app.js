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
  if (screen === "main") loadTracks();
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

// --- Music: the Music Box's tracks from the game data, with playlists, shuffle and repeat ---

// source: the playing track; fade: its own gain, for crossfades and its fade-out; buffer: the decoded track, to seek
// in it; loop: the track's own loop points (the game loops it there); startedAt and offset: the audio clock when the
// source started and the position it started from (the clock stands still while paused); ends: when the track is
// done, on the same clock (null while repeating it); queue and at: track indices in play order and the playing one's
// place; queueList: the list the queue came from; list: the shown list (null for all tracks, else a playlist's name)
const music = {
  lang: null, tracks: [], index: -1, ctx: null, gain: null, source: null, fade: null, buffer: null, loop: null,
  startedAt: 0, offset: 0, ends: null, timer: null, paused: false, token: 0, seeking: false,
  queue: [], at: -1, queueList: null, list: null, adding: null, dragFrom: -1,
};
const remembered = {
  get(key, fallback) { try { return localStorage.getItem(key) ?? fallback; } catch { return fallback; } },
  set(key, value) { try { localStorage.setItem(key, value); } catch { /* private storage: not remembered */ } },
};

const LOOPS = 2; // a looping track goes through its loop this many times, then the next one starts
const FADE_OUT = 4; // seconds a looping track fades out over at its end

// seconds over which one track fades into the next (Settings > Music)
const crossfadeSeconds = () => state?.settings?.music_crossfade ?? 2;
// "all": the list starts over; "one": the track repeats; "off": the music stops after the last track
const repeatMode = () => remembered.get("repeat", "all");
const shuffleOn = () => remembered.get("shuffle", "off") === "on";
const playlists = () => (state.settings.music_playlists ??= []);
const playlistNamed = (name) => playlists().find((p) => p.name === name);

function showPlaylist(open) {
  $("#playlist").hidden = !open;
  $("#playlist-show").hidden = open;
  remembered.set("playlist", open ? "open" : "closed");
}

async function loadTracks() {
  const lang = selectedLanguage();
  if (!state.data_ready || !lang || lang === music.lang) return;
  music.lang = lang;
  stopMusic();
  Object.assign(music, { index: -1, buffer: null, offset: 0, queue: [], at: -1, queueList: null });
  if (music.list !== null && !playlistNamed(music.list)) music.list = null;
  try {
    music.tracks = await invoke("music_tracks", { lang });
    say($("#music-msg"), "");
  } catch (e) {
    music.tracks = [];
    say($("#music-msg"), String(e), "error");
  }
  renderTracks();
}

// The tracks of a list (null for all of them, else a playlist's name), as indices into music.tracks
function listTracks(list = music.list) {
  if (list === null) return music.tracks.map((_, i) => i);
  const files = playlistNamed(list)?.files ?? [];
  return files.map((file) => music.tracks.findIndex((t) => t.file === file)).filter((i) => i >= 0);
}

function shuffled(items) {
  const out = [...items];
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}

// Rebuilds the queue from its list, in order or shuffled, keeping the playing track (`current`) at its place
function requeue(current = music.queue[music.at]) {
  const tracks = listTracks(music.queueList);
  if (shuffleOn()) {
    music.queue = current === undefined ? shuffled(tracks) : [current, ...shuffled(tracks.filter((t) => t !== current))];
    music.at = current === undefined ? -1 : 0;
  } else {
    music.queue = tracks;
    music.at = tracks.indexOf(current);
  }
  scheduleNext();
}

// Plays track `i` and queues the shown list around it
function playFromList(i) {
  music.queueList = music.list;
  requeue(i);
  playTrack(i);
}

// Moves `by` places in the queue; `auto` when the track finished by itself, which stops after the last with repeat off
function skip(by, auto = false) {
  if (!music.queue.length) {
    const first = listTracks()[0];
    return first === undefined ? undefined : playFromList(first);
  }
  let at = music.at + by;
  if (at < 0 || at >= music.queue.length) {
    if (auto && repeatMode() === "off") {
      stopMusic();
      music.offset = 0;
      return renderTracks();
    }
    if (at >= music.queue.length && shuffleOn()) {
      music.queue = shuffled(music.queue);
      at = 0;
    }
    at = (at + music.queue.length) % music.queue.length;
  }
  music.at = at;
  playTrack(music.queue[at]);
}

function renderTracks() {
  const list = $("#tracks");
  const query = $("#music-search").value.trim().toLowerCase();
  const inPlaylist = music.list !== null;
  const shown = listTracks().map((i, at) => [music.tracks[i], i, at]).filter(([track]) => track.title.toLowerCase().includes(query));
  list.replaceChildren(...shown.map(([track, i, at]) => {
    const item = document.createElement("li");
    item.value = at + 1; // keeps the track's number while searching
    item.className = i === music.index ? "current" : "";
    const button = document.createElement("button");
    button.textContent = track.title;
    button.addEventListener("click", () => playFromList(i));
    const edit = document.createElement("button");
    edit.className = "track-edit";
    if (inPlaylist) {
      edit.innerHTML = "&times;";
      edit.title = "Remove from the playlist";
      edit.addEventListener("click", () => editPlaylist(music.list, (files) => files.splice(at, 1)));
      // drag a row onto another to move it there
      item.draggable = true;
      item.addEventListener("dragstart", () => { music.dragFrom = at; });
      item.addEventListener("dragover", (e) => e.preventDefault());
      item.addEventListener("drop", (e) => {
        e.preventDefault();
        const from = music.dragFrom;
        if (from >= 0 && from !== at) editPlaylist(music.list, (files) => files.splice(at, 0, ...files.splice(from, 1)));
      });
    } else {
      edit.textContent = "+";
      edit.title = "Add to a playlist";
      edit.addEventListener("click", (e) => openAddMenu(e.currentTarget, track.file));
    }
    edit.setAttribute("aria-label", edit.title);
    item.append(button, edit);
    return item;
  }));
  if (inPlaylist && !shown.length && !query) {
    const empty = document.createElement("li");
    empty.className = "empty";
    empty.textContent = "Empty. Add tracks with + in All tracks.";
    list.append(empty);
  }

  const picker = $("#music-list");
  picker.replaceChildren(new Option("All tracks", ""), ...playlists().map((p) => new Option(p.name, p.name)));
  picker.value = music.list ?? "";
  $("#playlist-rename").hidden = $("#playlist-delete").hidden = !inPlaylist;

  $("#now-title").textContent = music.tracks[music.index]?.title ?? "Pick a track";
  $("#music-toggle").innerHTML = music.source && !music.paused ? "&#10074;&#10074;" : "&#9654;";
  const repeat = repeatMode();
  const repeatButton = $("#music-repeat");
  repeatButton.classList.toggle("on", repeat !== "off");
  repeatButton.classList.toggle("one", repeat === "one");
  repeatButton.title = { all: "Repeat: the list", one: "Repeat: this track", off: "Repeat: off" }[repeat];
  $("#music-shuffle").classList.toggle("on", shuffleOn());
  $("#music-shuffle").title = shuffleOn() ? "Shuffle: on" : "Shuffle: off";
  renderPosition();
}

const clock = (seconds) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;

// The loop the playing source repeats: the track's own, or the whole track while repeating it
const sourceLoop = () => (repeatMode() === "one" ? (music.loop ?? { start: 0, end: music.buffer.duration }) : music.loop);

// Where the playing track is, in seconds; a looping track wraps back to its loop start
function position() {
  if (!music.source) return music.offset;
  let t = music.offset + music.ctx.currentTime - music.startedAt;
  const loop = sourceLoop();
  if (loop && t >= loop.end) t = loop.start + ((t - loop.start) % (loop.end - loop.start));
  return Math.min(t, music.buffer.duration);
}

function renderPosition() {
  const seek = $("#music-seek");
  const length = music.buffer?.duration ?? 0;
  seek.disabled = !music.buffer;
  if (!music.seeking) {
    seek.value = length ? Math.round((position() / length) * 1000) : 0;
    $("#music-time").textContent = clock(length ? position() : 0);
  }
  $("#music-length").textContent = clock(length);
}
setInterval(() => { if (music.source && !music.paused) renderPosition(); }, 250);

// Stops the playing track, fading it out over `fade` seconds
function stopMusic(fade = 0) {
  clearTimeout(music.timer);
  if (!music.source) return;
  const { source, fade: gain } = music;
  source.onended = null;
  const now = music.ctx.currentTime;
  if (fade > 0) {
    gain.gain.cancelScheduledValues(now);
    gain.gain.setValueAtTime(gain.gain.value, now);
    gain.gain.linearRampToValueAtTime(0, now + fade);
    source.stop(now + fade);
  } else {
    source.stop();
  }
  music.source = null;
}

// Plays the decoded track from `offset` seconds, fading in over `fade` seconds
function startSource(offset, fade) {
  const source = music.ctx.createBufferSource();
  source.buffer = music.buffer;
  const loop = sourceLoop();
  if (loop) {
    source.loop = true;
    source.loopStart = loop.start;
    source.loopEnd = loop.end;
  }
  const gain = music.ctx.createGain();
  const now = music.ctx.currentTime;
  if (fade > 0) {
    gain.gain.setValueAtTime(0, now);
    gain.gain.linearRampToValueAtTime(1, now + fade);
  }
  // seconds left: to the end, or LOOPS times through the loop and a fade-out; none while repeating the track
  let left = null;
  if (repeatMode() !== "one") {
    left = music.buffer.duration - offset;
    if (music.loop) {
      const { start, end } = music.loop;
      left = Math.max(end + (LOOPS - 1) * (end - start) - offset, FADE_OUT);
      gain.gain.setValueAtTime(1, Math.max(now + fade, now + left - FADE_OUT));
      gain.gain.linearRampToValueAtTime(0, now + left);
    }
  }
  gain.connect(music.gain);
  source.connect(gain);
  source.onended = () => { if (music.source === source) { music.source = null; skip(1, true); } };
  source.start(now, offset);
  Object.assign(music, { source, fade: gain, startedAt: now, offset, ends: left === null ? null : now + left });
  scheduleNext();
}

// The next track starts a crossfade before this one is done; after the last one with repeat off, the music stops
function scheduleNext() {
  clearTimeout(music.timer);
  if (music.ends === null || !music.source || music.paused) return;
  const source = music.source;
  const last = repeatMode() === "off" && music.at >= music.queue.length - 1;
  const wait = Math.max(0, music.ends - (last ? 0 : crossfadeSeconds()) - music.ctx.currentTime);
  music.timer = setTimeout(() => { if (music.source === source && !music.paused) skip(1, true); }, wait * 1000);
}

function pauseMusic() {
  if (music.source && !music.paused) {
    clearTimeout(music.timer);
    music.ctx.suspend();
    music.paused = true;
    renderTracks();
  }
}

function setVolume() {
  const value = Number($("#music-volume").value);
  remembered.set("volume", String(value));
  if (music.gain) music.gain.gain.value = (value / 100) ** 2;
}

// Plays track `i` of music.tracks; the queue is the caller's
async function playTrack(i) {
  const track = music.tracks[i];
  if (!track) return;
  const token = ++music.token;
  if (music.source) music.source.onended = null; // the old track ending while this one loads moves on no further
  music.index = i;
  renderTracks();
  try {
    const bytes = await invoke("music_track", { lang: music.lang, file: track.file });
    if (token !== music.token) return;
    music.ctx ??= new AudioContext();
    if (!music.gain) {
      music.gain = music.ctx.createGain();
      music.gain.connect(music.ctx.destination);
      setVolume();
    }
    // 12 bytes of loop points in samples, then the Ogg Opus stream (see launcher/src/music.rs)
    // the raw response arrives as an ArrayBuffer, a typed array or a plain array depending on the transport
    const data = bytes instanceof ArrayBuffer ? new Uint8Array(bytes)
      : ArrayBuffer.isView(bytes) ? new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength)
      : Uint8Array.from(bytes);
    const head = new DataView(data.buffer, data.byteOffset, 12);
    const [loopStart, loopEnd, rate] = [0, 4, 8].map((offset) => head.getUint32(offset, true));
    const buffer = await music.ctx.decodeAudioData(data.slice(12).buffer);
    if (token !== music.token) return;
    // a track that is playing fades out while this one fades in; from silence or a pause it just starts
    const fade = music.source && !music.paused ? crossfadeSeconds() : 0;
    stopMusic(fade);
    music.buffer = buffer;
    music.loop = track.looping && loopEnd > loopStart ? { start: loopStart / rate, end: loopEnd / rate } : null;
    music.paused = false;
    startSource(0, fade);
    await music.ctx.resume();
  } catch (e) {
    say($("#music-msg"), String(e), "error");
  }
  renderTracks();
}

// Moves the playing (or paused) track to `seconds`
function seekTo(seconds) {
  if (!music.buffer) return;
  const paused = music.paused;
  stopMusic();
  music.paused = paused;
  startSource(Math.min(seconds, music.buffer.duration - 0.05), 0);
  renderTracks();
}

// --- Playlists, kept in launcher.json as names and track files ---

function savePlaylists() {
  if (music.queueList !== null && playlistNamed(music.queueList)) requeue();
  renderTracks();
  saveSettings().catch((e) => say($("#music-msg"), String(e), "error"));
}

// Applies `change` to the files of the playlist `name`
function editPlaylist(name, change) {
  const playlist = playlistNamed(name);
  if (!playlist) return;
  change(playlist.files);
  savePlaylists();
}

function showList(name) {
  music.list = name !== null && playlistNamed(name) ? name : null;
  remembered.set("music-list", music.list ?? "");
  $("#music-search").value = "";
  renderTracks();
}

// The name form: "new" (with `music.adding`, the track to put in it, if any) or "rename"
function openNameForm(mode) {
  const form = $("#playlist-form");
  form.dataset.mode = mode;
  form.hidden = false;
  $("#playlist-name").value = mode === "rename" ? music.list : "";
  $("#playlist-name").focus();
  closeAddMenu();
}

function closeNameForm() {
  $("#playlist-form").hidden = true;
  music.adding = null;
}

$("#playlist-form").addEventListener("submit", (e) => {
  e.preventDefault();
  const name = $("#playlist-name").value.trim();
  const mode = e.currentTarget.dataset.mode;
  const taken = playlists().some((p) => p.name.toLowerCase() === name.toLowerCase() && !(mode === "rename" && p.name === music.list));
  if (!name || taken) {
    say($("#music-msg"), name ? "There is already a playlist with that name." : "Type a name.", "error");
    return;
  }
  say($("#music-msg"), "");
  if (mode === "rename") {
    const old = music.list;
    playlistNamed(old).name = name;
    if (music.queueList === old) music.queueList = name;
    music.list = name;
    remembered.set("music-list", name);
  } else {
    playlists().push({ name, files: music.adding ? [music.adding] : [] });
    if (!music.adding) showList(name);
  }
  closeNameForm();
  savePlaylists();
});
$("#playlist-cancel").addEventListener("click", closeNameForm);
$("#playlist-new").addEventListener("click", () => openNameForm("new"));
$("#playlist-rename").addEventListener("click", () => openNameForm("rename"));
// a first click asks, a second within 3 seconds deletes
$("#playlist-delete").addEventListener("click", (e) => {
  const button = e.currentTarget;
  if (!button.dataset.armed) {
    button.dataset.armed = "1";
    button.textContent = "Sure?";
    setTimeout(() => { delete button.dataset.armed; button.textContent = "Delete"; }, 3000);
    return;
  }
  const name = music.list;
  state.settings.music_playlists = playlists().filter((p) => p.name !== name);
  if (music.queueList === name) music.queueList = null; // what is playing goes on through all tracks
  showList(null);
  savePlaylists();
});
$("#music-list").addEventListener("change", (e) => showList(e.target.value || null));

// The menu under a track's + button: each playlist, then a new one
function openAddMenu(anchor, file) {
  const menu = $("#add-menu");
  const items = playlists().map((p) => {
    const item = document.createElement("button");
    const has = p.files.includes(file);
    item.textContent = has ? `${p.name} ✓` : p.name;
    item.disabled = has;
    item.addEventListener("click", () => { closeAddMenu(); editPlaylist(p.name, (files) => files.push(file)); });
    return item;
  });
  const fresh = document.createElement("button");
  fresh.textContent = "New playlist…";
  fresh.addEventListener("click", () => { openNameForm("new"); music.adding = file; });
  menu.replaceChildren(...items, fresh);
  menu.hidden = false;
  const panel = $("#playlist").getBoundingClientRect();
  const at = anchor.getBoundingClientRect();
  menu.style.top = `${at.bottom - panel.top + 2}px`;
  menu.style.right = `${panel.right - at.right}px`;
}
const closeAddMenu = () => { $("#add-menu").hidden = true; };
document.addEventListener("click", (e) => {
  if (!e.target.closest("#add-menu, .track-edit")) closeAddMenu();
});
$("#tracks").addEventListener("scroll", closeAddMenu);

// --- Controls ---

$("#music-toggle").addEventListener("click", () => {
  if (!music.source) return music.index >= 0 ? playTrack(music.index) : skip(1);
  if (music.paused) {
    music.ctx.resume();
    music.paused = false;
    scheduleNext();
    renderTracks();
  } else {
    pauseMusic();
  }
});
// Previous starts the track over when it is more than 3 seconds in
$("#music-prev").addEventListener("click", () => (music.source && position() > 3 ? seekTo(0) : skip(-1)));
$("#music-next").addEventListener("click", () => skip(1));
$("#music-repeat").addEventListener("click", () => {
  remembered.set("repeat", { all: "one", one: "off", off: "all" }[repeatMode()]);
  if (music.source) seekTo(position()); // the source picks up the new loop and end
  else renderTracks();
});
$("#music-shuffle").addEventListener("click", () => {
  remembered.set("shuffle", shuffleOn() ? "off" : "on");
  if (music.queue.length) requeue();
  renderTracks();
});
$("#music-volume").value = remembered.get("volume", "60");
$("#music-volume").addEventListener("input", setVolume);
$("#music-search").addEventListener("input", renderTracks);
// dragging shows the time under the thumb; letting go jumps there
$("#music-seek").addEventListener("input", (e) => {
  music.seeking = true;
  $("#music-time").textContent = clock((e.target.value / 1000) * (music.buffer?.duration ?? 0));
});
$("#music-seek").addEventListener("change", (e) => {
  music.seeking = false;
  seekTo((e.target.value / 1000) * (music.buffer?.duration ?? 0));
});
$("#music-crossfade").addEventListener("change", (e) => {
  state.settings.music_crossfade = Number(e.target.value);
  saveSettings();
});
$("#playlist-hide").addEventListener("click", () => showPlaylist(false));
$("#playlist-show").addEventListener("click", () => showPlaylist(true));
showPlaylist(remembered.get("playlist", "open") === "open");
// the shown list; loadTracks drops it if no playlist has that name
music.list = remembered.get("music-list", "") || null;

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
    pauseMusic();
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
  $("#music-crossfade").value = String(state.settings.music_crossfade ?? 2);
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

$("#import-save").addEventListener("click", async () => {
  const file = await dialog.open({ title: "Choose your SAVEDATA file" });
  if (!file) return;
  const msg = $("#settings-msg");
  try {
    await invoke("import_save", { file });
    say(msg, "Save imported. The old one was kept as SAVEDATA.bak.", "ok");
  } catch (e) {
    say(msg, String(e), "error");
  }
});

// --- Launcher updates ---

let pendingUpdate = null;

// manual: say "up to date" or the error in `msg`; an automatic check's failure (offline, no release yet) stays silent
let lastUpdateCheck = 0;
async function checkAppUpdate(manual, msg = $("#app-update-msg")) {
  lastUpdateCheck = Date.now();
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

for (const [button, msg] of [["#check-app-update", "#app-update-msg"], ["#history-check-update", "#history-update-msg"]]) {
  $(button).addEventListener("click", async (e) => {
    e.target.disabled = true;
    await checkAppUpdate(true, $(msg));
    e.target.disabled = false;
  });
}

// while the launcher stays open: again every hour, and when its window comes back into focus after 10 minutes
const autoCheck = () => {
  if (!state?.program?.uninstall && !pendingUpdate && Date.now() - lastUpdateCheck > 10 * 60 * 1000) checkAppUpdate(false);
};
window.addEventListener("focus", autoCheck);
setInterval(autoCheck, 60 * 60 * 1000);

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
