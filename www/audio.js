// Sound playback.
//
// All sounds are fetched and decoded upfront (`load_sound`) and played with a fresh
// `AudioBufferSourceNode` per playback (`play_sound`), so overlapping sounds - including
// overlapping instances of the same sound - mix naturally and always play to completion.
//
// Sounds triggered (almost) simultaneously are spaced `SOUND_MIN_INTERVAL_S` apart to keep
// each audibly distinct, e.g. when the opponent moves and a premove is executed in response.
// The spacing is scheduled on the audio clock (`source.start(time)`) rather than with
// `setTimeout`: this is sample-accurate and immune to background tab timer throttling.
//
// Browsers refuse to produce sound until the user has interacted with the page ("autoplay
// policy"): an `AudioContext` created before the first interaction starts out "suspended"
// and must be `resume()`d. A `resume()` call is guaranteed to succeed only inside a user
// gesture handler, so we arm one-shot document-level listeners that unlock audio on the
// first click or keypress. The context can also become suspended mid-session (e.g. on
// output device change or OS-level interruption); the "statechange" listener re-arms the
// unlock listeners whenever this happens.
//
// Improvement potential. Establish priority on sounds; play more important sounds first
// in case of a clash.

import { fetch_asset } from "./assets";

// Min delay between two sounds. Small enough to feel instant, large enough to hear two
// overlapping sounds as separate events.
const SOUND_MIN_INTERVAL_S = 0.07;

// If a burst of sounds doesn't fit into the schedule, drop the excess rather than play
// sounds long after the events that triggered them.
const MAX_SCHEDULE_AHEAD_S = 0.5;

const audio_context = new AudioContext({ latencyHint: "interactive" });
const master_gain_node = audio_context.createGain();
master_gain_node.connect(audio_context.destination);

const sounds = new Map(); // name -> AudioBuffer
const missing_sounds_reported = new Set();
let next_sound_time = 0; // on the `audio_context.currentTime` timeline

function unlock_audio() {
  audio_context.resume();
}

function arm_audio_unlock() {
  // Note. `addEventListener` deduplicates listeners, so re-arming is always safe.
  for (const event of ["pointerdown", "keydown"]) {
    document.addEventListener(event, unlock_audio, { once: true, capture: true });
  }
}

arm_audio_unlock();
audio_context.addEventListener("statechange", () => {
  if (audio_context.state !== "running") {
    // Try to resume directly: works if the user has already interacted with the page.
    // If the browser refuses, the next click or keypress will unlock audio.
    audio_context.resume().catch(() => {});
    arm_audio_unlock();
  }
});

export async function load_sound(name, url) {
  const array_buffer = await fetch_asset(url);
  sounds.set(name, await audio_context.decodeAudioData(array_buffer));
}

// `volume` is 0.0 (silence) to 1.0. Sounds are still technically played when muted.
export function set_sound_volume(volume) {
  master_gain_node.gain.value = volume;
}

// Plays a preloaded sound. `pan` is -1.0 (left) to 1.0 (right).
export function play_sound(name, pan = 0) {
  if (audio_context.state !== "running") {
    // Backup for the unlock listeners. Sources scheduled on a suspended context are not
    // lost: if this `resume` (or a later one) succeeds, the sound plays upon resumption.
    audio_context.resume().catch(() => {});
  }
  const buffer = sounds.get(name);
  if (buffer === undefined) {
    if (!missing_sounds_reported.has(name)) {
      missing_sounds_reported.add(name);
      console.error(`Cannot play sound "${name}": not loaded. Further errors suppressed.`);
    }
    return;
  }
  const now = audio_context.currentTime;
  const start_time = Math.max(now, next_sound_time);
  if (start_time > now + MAX_SCHEDULE_AHEAD_S) {
    return;
  }
  next_sound_time = start_time + SOUND_MIN_INTERVAL_S;
  const source_node = audio_context.createBufferSource();
  source_node.buffer = buffer;
  const panner_node = audio_context.createStereoPanner();
  panner_node.pan.value = pan;
  source_node.connect(panner_node);
  panner_node.connect(master_gain_node);
  source_node.start(start_time);
}
