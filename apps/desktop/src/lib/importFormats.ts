/**
 * importFormats — which audio files the app can open on this computer.
 *
 * This repeats `audio_decoder::SUPPORTED_EXTENSIONS` and
 * `audio_decoder::OS_DECODER_EXTENSIONS`;
 * `crates/audio-decoder/tests/supported_extensions.rs` fails if a copy
 * differs. A type the picker offers but the decoder cannot open fails after
 * the person chose it, so nothing is offered that is not opened.
 *
 * M4A/AAC is opened by the operating system's own decoder (AudioToolbox on
 * macOS, Media Foundation on Windows, #764), because an AAC decoder built
 * into the app would need a patent licence the project does not hold. Linux
 * has none, so the picker and the landing screen offer M4A on macOS and
 * Windows only. Opus is opened everywhere, by a pure-Rust codec.
 */

/** Opened on every platform. */
export const BASE_EXTENSIONS = ["wav", "mp3", "flac", "ogg", "opus", "aif", "aiff"];

/** Opened only where the operating system has a decoder: macOS and Windows. */
export const OS_DECODER_EXTENSIONS = ["m4a"];

/** The formats' short names, as the landing screen says them (`aif` and `aiff` are one format). */
export const BASE_FORMAT_NAMES = ["wav", "mp3", "flac", "ogg", "opus", "aiff"];

/**
 * Whether this computer's own decoder handles M4A/AAC: macOS and Windows.
 *
 * `navigator.platform` is deprecated but is what is reliably available inside
 * the webview (`userAgentData` is not implemented in WKWebView), and the
 * landing screen's shortcut hint reads it the same way. The match is
 * case-sensitive on purpose: a Linux user agent can hold "darwin"-like words
 * in lower case, never "Mac" or "Win" in this spelling.
 */
export function osDecodesM4a(): boolean {
  if (typeof navigator === "undefined") return false;
  const platform = `${navigator.platform ?? ""} ${navigator.userAgent ?? ""}`;
  return /Mac|Win/.test(platform) && !/iPhone|iPad|Android/.test(platform);
}

/** The extensions the file picker offers on this computer. */
export function importExtensions(): string[] {
  return osDecodesM4a()
    ? [...BASE_EXTENSIONS, ...OS_DECODER_EXTENSIONS]
    : [...BASE_EXTENSIONS];
}

/** The format names the landing screen lists on this computer. */
export function importFormatNames(): string[] {
  return osDecodesM4a()
    ? [...BASE_FORMAT_NAMES, ...OS_DECODER_EXTENSIONS]
    : [...BASE_FORMAT_NAMES];
}
