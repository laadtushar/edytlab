/**
 * Canvas — left-pane waveform view backed by wavesurfer.js v7.
 *
 * Responsibilities:
 *  - Show an empty-state prompt until a file is loaded.
 *  - Accept native drag-and-drop of an audio file. The drop is converted
 *    to a `load <path>` chat message routed through the bridge so the
 *    agent owns the actual project mutation (M11 does not expose a
 *    direct "load file" command — the chat path is the documented
 *    pointer in the M12 spec).
 *  - Render a waveform from a Tauri-resolved file URL when `audioPath`
 *    is set (e.g. by Render Preview).
 *  - Spacebar play/pause shortcut while focused (or anywhere on the
 *    document, scoped via a window listener that ignores typing).
 *
 * The wavesurfer instance is owned by a `useRef` so re-renders do not
 * tear it down. We destroy it only on unmount or when the container
 * element changes.
 */

import { useCallback, useEffect, useRef, useState } from "react";

import WaveSurfer from "wavesurfer.js";
import { convertFileSrc } from "@tauri-apps/api/core";

import { sendMessage as bridgeSendMessage } from "../lib/tauri-bridge";

export interface CanvasProps {
  /** Absolute path of an audio file to render (e.g. a render-preview
   * temp WAV). When this changes the waveform is reloaded. */
  audioPath: string | null;
  /** Called when the user drops an audio file onto the canvas. The
   * parent may use this to update local UI state in addition to the
   * chat message routing handled here. */
  onFileDropped?: (path: string) => void;
}

export function Canvas({ audioPath, onFileDropped }: CanvasProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const wavesurferRef = useRef<WaveSurfer | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);

  // Initialise WaveSurfer once the container mounts.
  useEffect(() => {
    if (!containerRef.current) return;
    const ws = WaveSurfer.create({
      container: containerRef.current,
      waveColor: "#4b5563",
      progressColor: "#3b82f6",
      cursorColor: "#e5e7eb",
      cursorWidth: 1,
      height: 160,
      barWidth: 2,
      barGap: 1,
      barRadius: 1,
      normalize: true,
    });
    wavesurferRef.current = ws;
    const onPlay = () => setIsPlaying(true);
    const onPause = () => setIsPlaying(false);
    const onFinish = () => setIsPlaying(false);
    ws.on("play", onPlay);
    ws.on("pause", onPause);
    ws.on("finish", onFinish);

    return () => {
      ws.un("play", onPlay);
      ws.un("pause", onPause);
      ws.un("finish", onFinish);
      ws.destroy();
      wavesurferRef.current = null;
    };
  }, []);

  // Load a new audio file whenever `audioPath` changes.
  useEffect(() => {
    const ws = wavesurferRef.current;
    if (!ws || !audioPath) return;
    setLoadError(null);
    try {
      const url = convertFileSrc(audioPath);
      ws.load(url).catch((err: unknown) => {
        setLoadError(String(err));
      });
    } catch (err) {
      setLoadError(String(err));
    }
  }, [audioPath]);

  const togglePlay = useCallback(() => {
    const ws = wavesurferRef.current;
    if (!ws) return;
    void ws.playPause();
  }, []);

  // Spacebar shortcut. Ignored when an input or textarea is focused so
  // the user can type spaces in the chat box.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.code !== "Space") return;
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === "INPUT" ||
          t.tagName === "TEXTAREA" ||
          t.isContentEditable)
      ) {
        return;
      }
      e.preventDefault();
      togglePlay();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [togglePlay]);

  const handleDrop = useCallback(
    async (e: React.DragEvent<HTMLDivElement>) => {
      e.preventDefault();
      setIsDragging(false);
      const file = e.dataTransfer.files?.[0];
      if (!file) return;
      // In a Tauri webview the dropped file's `path` is exposed on the
      // file object as a non-standard property; in dev / browsers we
      // fall back to the file name and let the agent resolve it.
      const path =
        (file as File & { path?: string }).path ?? file.name;
      onFileDropped?.(path);
      try {
        await bridgeSendMessage(`load this file: ${path}`);
      } catch (err) {
        setLoadError(String(err));
      }
    },
    [onFileDropped],
  );

  const handleDragOver = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    if (!isDragging) setIsDragging(true);
  };

  const handleDragLeave = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setIsDragging(false);
  };

  return (
    <div
      data-testid="canvas-root"
      onDrop={handleDrop}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      className={
        "relative flex h-full w-full flex-col bg-neutral-900 " +
        (isDragging ? "ring-2 ring-blue-500" : "")
      }
    >
      <div className="flex items-center justify-between border-b border-neutral-800 px-3 py-2">
        <h2 className="text-sm font-semibold text-neutral-100">Canvas</h2>
        <button
          type="button"
          data-testid="play-pause-button"
          onClick={togglePlay}
          className="rounded-md border border-neutral-700 bg-neutral-800 px-2 py-1 text-xs text-neutral-100 hover:bg-neutral-700"
          aria-label={isPlaying ? "Pause" : "Play"}
        >
          {isPlaying ? "Pause" : "Play"}
        </button>
      </div>

      <div className="relative flex-1 px-3 py-3">
        <div
          ref={containerRef}
          data-testid="wavesurfer-container"
          className="h-full w-full"
        />
        {!audioPath ? (
          <div
            data-testid="canvas-empty"
            className="pointer-events-none absolute inset-0 flex items-center justify-center px-3 py-3 text-center text-sm text-neutral-400"
          >
            Drop an audio file or use the file menu
          </div>
        ) : null}
        {loadError ? (
          <div
            data-testid="canvas-error"
            role="alert"
            className="absolute bottom-3 left-3 right-3 rounded-md border border-red-800 bg-red-900/60 px-3 py-2 text-xs text-red-100"
          >
            {loadError}
          </div>
        ) : null}
      </div>
    </div>
  );
}
