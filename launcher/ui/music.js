// The launcher's music player: the Music Box's tracks from the game data, with playlists, shuffle and repeat.
// It is apart from the rest of the launcher: its own settings (music.json, through music_settings and
// save_music_settings) and its own rules for playing a track; the launcher only tells it the language
// (musicPlayer.setLanguage) and that the game is starting (musicPlayer.gameStarted). Uses $, say and invoke from app.js.

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

// the player's settings (Settings > Music, and the playlists); see music::Settings in launcher/src/music.rs
const musicSettings = { crossfade: 2, loops: 1, fade_out: 4, keep_playing: false, playlists: [] };
const crossfadeSeconds = () => musicSettings.crossfade;
const saveMusicSettings = () => invoke("save_music_settings", { settings: musicSettings });
// "all": the list starts over; "one": the track repeats; "off": the music stops after the last track
const repeatMode = () => remembered.get("repeat", "all");
const shuffleOn = () => remembered.get("shuffle", "off") === "on";
const playlists = () => musicSettings.playlists;
const playlistNamed = (name) => playlists().find((p) => p.name === name);

function showPlaylist(open) {
  $("#playlist").hidden = !open;
  $("#playlist-show").hidden = open;
  remembered.set("playlist", open ? "open" : "closed");
}

// The track list in `lang` (its titles); what is playing goes on when the files are the same, as they are
// between the game's languages
async function loadTracks(lang) {
  if (!lang || lang === music.lang) return;
  music.lang = lang;
  let tracks = [];
  try {
    tracks = await invoke("music_tracks", { lang });
    say($("#music-msg"), "");
  } catch (e) {
    say($("#music-msg"), String(e), "error");
  }
  const same = tracks.length === music.tracks.length && tracks.every((t, i) => t.file === music.tracks[i].file);
  if (!same) {
    stopMusic();
    Object.assign(music, { index: -1, buffer: null, offset: 0, queue: [], at: -1, queueList: null });
  }
  music.tracks = tracks;
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
  // seconds left: to the end, or through the loop as many times as set, fading out over the last few; none while
  // repeating the track
  let left = null;
  if (repeatMode() !== "one") {
    left = music.buffer.duration - offset;
    if (music.loop) {
      const { start, end } = music.loop;
      const fadeOut = musicSettings.fade_out;
      left = Math.max(end + (Math.max(musicSettings.loops, 1) - 1) * (end - start) - offset, fadeOut, 0.05);
      if (fadeOut > 0) {
        gain.gain.setValueAtTime(1, Math.max(now + fade, now + left - fadeOut));
        gain.gain.linearRampToValueAtTime(0, now + left);
      }
    }
  }
  gain.connect(music.gain);
  source.connect(gain);
  source.onended = () => { if (music.source === source) { music.source = null; skip(1, true); } };
  source.start(now, offset);
  if (left !== null) source.stop(now + left); // a looping source would go round again while the next one loads
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
    music.loop = loopEnd > loopStart ? { start: loopStart / rate, end: loopEnd / rate } : null;
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

// --- Playlists, kept in music.json as names and track files ---

function savePlaylists() {
  if (music.queueList !== null && playlistNamed(music.queueList)) requeue();
  renderTracks();
  saveMusicSettings().catch((e) => say($("#music-msg"), String(e), "error"));
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
  musicSettings.playlists = playlists().filter((p) => p.name !== name);
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

// --- Settings > Music ---

function renderMusicSettings() {
  $("#music-crossfade").value = String(musicSettings.crossfade);
  $("#music-loops").value = String(musicSettings.loops);
  $("#music-fade-out").value = String(musicSettings.fade_out);
  $("#music-keep-playing").checked = musicSettings.keep_playing;
}

// field, its setting, and whether it changes how the playing track ends
for (const [id, key, replan] of [
  ["#music-crossfade", "crossfade", false], ["#music-loops", "loops", true], ["#music-fade-out", "fade_out", true],
  ["#music-keep-playing", "keep_playing", false],
]) {
  $(id).addEventListener("change", (e) => {
    musicSettings[key] = e.target.type === "checkbox" ? e.target.checked : Number(e.target.value);
    saveMusicSettings().catch((err) => say($("#settings-msg"), String(err), "error"));
    if (replan && music.source) seekTo(position());
    else scheduleNext();
  });
}
$("#playlist-hide").addEventListener("click", () => showPlaylist(false));
$("#playlist-show").addEventListener("click", () => showPlaylist(true));
showPlaylist(remembered.get("playlist", "open") === "open");
// the settings, then the shown list (dropped if no playlist has that name)
invoke("music_settings")
  .then((saved) => Object.assign(musicSettings, saved))
  .catch((e) => say($("#music-msg"), String(e), "error"))
  .finally(() => {
    const list = remembered.get("music-list", "") || null;
    music.list = list !== null && playlistNamed(list) ? list : null;
    renderMusicSettings();
    renderTracks();
  });

// What the rest of the launcher tells the player
const musicPlayer = {
  // the selected language, or null while the game data is not set up
  setLanguage(lang) { loadTracks(lang); },
  gameStarted() { if (!musicSettings.keep_playing) pauseMusic(); },
};
