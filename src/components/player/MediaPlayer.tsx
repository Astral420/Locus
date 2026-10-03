import React, { useRef, useState, useEffect, useCallback } from "react";
import {
  Play,
  Pause,
  RotateCcw,
  RotateCw,
  Volume2,
  VolumeX,
  Maximize,
  Minimize,
  Sliders,
  AudioWaveform as Waveform,
  Mic,
  Monitor,
} from "lucide-react";
import { safeConvertFileSrc } from "../../lib/tauri";

export interface MediaPlayerProps {
  src?: string;
  hasVideo: boolean;
  meetingTitle: string;
  durationSeconds: number;
  currentTime: number;
  onTimeUpdate: (time: number) => void;
  onSeek?: (time: number) => void;
  onDurationChange?: (duration: number) => void;
}

const PLAYBACK_SPEEDS = [0.75, 1.0, 1.25, 1.5, 2.0];

export const MediaPlayer: React.FC<MediaPlayerProps> = ({
  src,
  hasVideo,
  meetingTitle,
  durationSeconds,
  currentTime,
  onTimeUpdate,
  onSeek,
  onDurationChange,
}) => {
  const videoRef = useRef<HTMLVideoElement | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);

  const [isPlaying, setIsPlaying] = useState(false);
  const [playbackSpeed, setPlaybackSpeed] = useState(1.0);
  const [volume, setVolume] = useState(1.0);
  const [isMuted, setIsMuted] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [isScrubbing, setIsScrubbing] = useState(false);
  const [hoverTime, setHoverTime] = useState<number | null>(null);
  const [showSpeedMenu, setShowSpeedMenu] = useState(false);
  const [mediaError, setMediaError] = useState<string | null>(null);

  // Synchronize converted src
  const mediaSrc = src ? safeConvertFileSrc(src) : undefined;

  // Reset any previous load error when the source changes
  useEffect(() => {
    setMediaError(null);
  }, [mediaSrc]);

  const handleMediaError = (e: React.SyntheticEvent<HTMLMediaElement>) => {
    const code = e.currentTarget.error?.code;
    setIsPlaying(false);
    setMediaError(
      code === 4
        ? "This recording's format or location can't be played by the app."
        : "Playback failed while loading the recording."
    );
  };

  const formatTime = (secs: number) => {
    if (isNaN(secs) || secs < 0) secs = 0;
    const m = Math.floor(secs / 60);
    const s = Math.floor(secs % 60);
    return `${m.toString().padStart(2, "0")}:${s.toString().padStart(2, "0")}`;
  };

  // Toggle Play / Pause
  const togglePlay = useCallback(() => {
    const el = hasVideo ? videoRef.current : audioRef.current;
    if (el) {
      if (el.paused) {
        el.play().then(
          () => setIsPlaying(true),
          () => setIsPlaying(false)
        );
      } else {
        el.pause();
        setIsPlaying(false);
      }
    } else {
      setIsPlaying((prev) => !prev);
    }
  }, [hasVideo]);

  // Seek helper (<200ms latency)
  const seekTo = useCallback(
    (seconds: number) => {
      const targetTime = Math.max(0, Math.min(seconds, durationSeconds || 1));
      const el = hasVideo ? videoRef.current : audioRef.current;
      if (el) {
        el.currentTime = targetTime;
      }
      onTimeUpdate(targetTime);
      onSeek?.(targetTime);
    },
    [hasVideo, durationSeconds, onTimeUpdate, onSeek]
  );

  // Jump relative seconds (±5s, ±15s)
  const jumpRelative = useCallback(
    (delta: number) => {
      seekTo(currentTime + delta);
    },
    [currentTime, seekTo]
  );

  // Speed selection
  const handleSpeedChange = (speed: number) => {
    setPlaybackSpeed(speed);
    setShowSpeedMenu(false);
    const el = hasVideo ? videoRef.current : audioRef.current;
    if (el) {
      el.playbackRate = speed;
    }
  };

  // Volume slider
  const handleVolumeChange = (newVolume: number) => {
    setVolume(newVolume);
    setIsMuted(newVolume === 0);
    const el = hasVideo ? videoRef.current : audioRef.current;
    if (el) {
      el.volume = newVolume;
      el.muted = newVolume === 0;
    }
  };

  // Toggle Mute
  const toggleMute = () => {
    const newMuted = !isMuted;
    setIsMuted(newMuted);
    const el = hasVideo ? videoRef.current : audioRef.current;
    if (el) {
      el.muted = newMuted;
    }
  };

  // Toggle Fullscreen
  const toggleFullscreen = () => {
    if (!containerRef.current) return;
    if (!document.fullscreenElement) {
      containerRef.current.requestFullscreen().catch(() => {});
      setIsFullscreen(true);
    } else {
      document.exitFullscreen().catch(() => {});
      setIsFullscreen(false);
    }
  };

  // Global Keyboard shortcuts when focusing player or studio (Space, Left/Right, Shift+Left/Right)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't trigger if user is typing in an input or textarea
      const target = e.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable) {
        return;
      }

      if (e.code === "Space") {
        e.preventDefault();
        togglePlay();
      } else if (e.code === "ArrowLeft") {
        e.preventDefault();
        jumpRelative(e.shiftKey ? -15 : -5);
      } else if (e.code === "ArrowRight") {
        e.preventDefault();
        jumpRelative(e.shiftKey ? 15 : 5);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [togglePlay, jumpRelative]);

  // Sync media currentTime if changed externally (e.g. clicking transcript line)
  useEffect(() => {
    const el = hasVideo ? videoRef.current : audioRef.current;
    if (el && Math.abs(el.currentTime - currentTime) > 0.5) {
      el.currentTime = currentTime;
    }
  }, [currentTime, hasVideo]);

  // Simulated playback timer for mock mode if media has no native events
  useEffect(() => {
    if (!isPlaying) return;
    const interval = setInterval(() => {
      const el = hasVideo ? videoRef.current : audioRef.current;
      if (el && !el.paused) {
        onTimeUpdate(el.currentTime);
      } else if (!el) {
        onTimeUpdate(Math.min(durationSeconds, currentTime + 1));
      }
    }, 1000);
    return () => clearInterval(interval);
  }, [isPlaying, hasVideo, currentTime, durationSeconds, onTimeUpdate]);

  const progressPercent = durationSeconds > 0 ? (currentTime / durationSeconds) * 100 : 0;

  return (
    <div
      ref={containerRef}
      tabIndex={0}
      role="region"
      aria-label="Media player master controls"
      className="relative flex flex-col rounded-lg border border-border bg-black/95 text-white shadow-sm overflow-hidden select-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
    >
      {/* Visual Canvas: Video Canvas OR Dedicated Audio Transport Card */}
      {hasVideo ? (
        <div className="relative aspect-video w-full bg-black flex items-center justify-center overflow-hidden">
          {mediaSrc ? (
            <video
              ref={videoRef}
              src={mediaSrc}
              className="w-full h-full object-contain"
              onTimeUpdate={(e) => onTimeUpdate(e.currentTarget.currentTime)}
              onDurationChange={(e) => onDurationChange?.(e.currentTarget.duration)}
              onEnded={() => setIsPlaying(false)}
              onPlay={() => setIsPlaying(true)}
              onPause={() => setIsPlaying(false)}
              onError={handleMediaError}
              onClick={togglePlay}
            />
          ) : (
            /* Synthetic Visual Screen Placeholder */
            <div
              className="absolute inset-0 flex flex-col items-center justify-center p-6 text-center bg-gradient-to-b from-stone-900 to-black cursor-pointer"
              onClick={togglePlay}
            >
              <div className="w-14 h-14 rounded-full bg-primary/20 border border-primary/30 flex items-center justify-center mb-3 text-primary group-hover:scale-105 transition-transform">
                <Monitor className="w-7 h-7" />
              </div>
              <p className="text-sm font-semibold text-stone-200 tracking-tight">{meetingTitle}</p>
              <p className="text-xs text-stone-400 mt-1 font-mono">
                Screen Recording · H.264 MP4 · 1080p @ 30fps
              </p>
            </div>
          )}
        </div>
      ) : (
        /* Dedicated Audio Transport Card (DESIGN.md §4.2.A: No blank video!) */
        <div
          className="relative w-full h-44 bg-gradient-to-br from-stone-950 via-[#141C18] to-stone-900 flex flex-col justify-between p-5 cursor-pointer border-b border-border/40"
          onClick={togglePlay}
        >
          <div className="flex items-start justify-between">
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-lg bg-primary/20 border border-primary/40 flex items-center justify-center text-primary">
                <Mic className="w-5 h-5" />
              </div>
              <div>
                <span className="text-[10px] font-mono tracking-wider uppercase text-primary font-semibold">
                  Audio-Only Session
                </span>
                <h3 className="text-sm font-semibold text-stone-100 line-clamp-1">{meetingTitle}</h3>
              </div>
            </div>

            <div className="flex items-center gap-2 text-[11px] font-mono text-stone-400 bg-stone-900/80 px-2.5 py-1 rounded border border-border/40">
              <span className="inline-block w-2 h-2 rounded-full bg-mint animate-pulse" />
              <span>{mediaSrc ? "Recorded audio" : "No audio file"}</span>
            </div>
          </div>

          {/* Calibrated Audio Waveform Visualization Graphic */}
          <div className="flex items-end justify-between gap-1 h-14 px-2" aria-hidden="true">
            {[
              24, 40, 18, 55, 78, 62, 35, 80, 95, 45, 60, 30, 75, 85, 40, 68, 92, 50, 38, 70,
              84, 48, 62, 90, 75, 42, 58, 88, 64, 32, 70, 85, 52, 36, 78, 90, 44, 60, 82, 35,
            ].map((height, i) => (
              <span
                key={i}
                className="w-1.5 rounded-t transition-all duration-150"
                style={{
                  height: isPlaying ? `${height}%` : "15%",
                  backgroundColor:
                    i % 3 === 0
                      ? "var(--color-mint)"
                      : i % 2 === 0
                      ? "var(--color-primary)"
                      : "rgba(255,255,255,0.25)",
                }}
              />
            ))}
          </div>

          <audio
            ref={audioRef}
            src={mediaSrc}
            onTimeUpdate={(e) => onTimeUpdate(e.currentTarget.currentTime)}
            onDurationChange={(e) => onDurationChange?.(e.currentTarget.duration)}
            onEnded={() => setIsPlaying(false)}
            onPlay={() => setIsPlaying(true)}
            onPause={() => setIsPlaying(false)}
            onError={handleMediaError}
          />
        </div>
      )}

      {mediaError && (
        <div role="alert" className="px-3 py-2 text-xs bg-red-950/60 text-red-200 border-t border-red-500/30">
          {mediaError}
        </div>
      )}

      {/* Control Room Scrubber Bar & Transport Controls */}
      <div className="p-3 bg-gradient-to-t from-stone-950 via-stone-900/90 to-stone-900/40 space-y-2 border-t border-white/5">
        {/* Scrubber Progress Bar Track */}
        <div
          role="slider"
          aria-label="Seek time slider"
          aria-valuemin={0}
          aria-valuemax={durationSeconds}
          aria-valuenow={currentTime}
          aria-valuetext={formatTime(currentTime)}
          tabIndex={0}
          className="group relative h-2 hover:h-2.5 bg-white/15 rounded-full cursor-pointer transition-all"
          onClick={(e) => {
            const rect = e.currentTarget.getBoundingClientRect();
            const pos = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
            seekTo(pos * durationSeconds);
          }}
          onMouseMove={(e) => {
            const rect = e.currentTarget.getBoundingClientRect();
            const pos = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
            setHoverTime(pos * durationSeconds);
          }}
          onMouseLeave={() => setHoverTime(null)}
        >
          {/* Progress Fill */}
          <div
            className="h-full bg-primary rounded-full relative transition-[width] duration-75"
            style={{ width: `${progressPercent}%` }}
          >
            {/* Luminous Mint Scrubber Thumb */}
            <span
              className="absolute right-0 top-1/2 -translate-y-1/2 w-3.5 h-3.5 bg-white border-2 border-mint rounded-full shadow-md scale-90 group-hover:scale-110 transition-transform"
              aria-hidden="true"
            />
          </div>

          {/* Hover Time Tooltip */}
          {hoverTime !== null && (
            <span
              className="absolute -top-7 -translate-x-1/2 px-1.5 py-0.5 rounded bg-stone-900 text-stone-200 text-[10px] font-mono border border-border shadow pointer-events-none"
              style={{
                left: `${(hoverTime / (durationSeconds || 1)) * 100}%`,
              }}
            >
              {formatTime(hoverTime)}
            </span>
          )}
        </div>

        {/* Master Controls Toolbar */}
        <div className="flex items-center justify-between text-xs pt-1">
          {/* Left Controls: Play/Pause, Rewind/Forward 5s, Tabular Time Readout */}
          <div className="flex items-center gap-2 sm:gap-3">
            <button
              type="button"
              onClick={togglePlay}
              className="p-1.5 rounded-md hover:bg-white/10 text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent transition-colors"
              aria-label={isPlaying ? "Pause playback (Space)" : "Start playback (Space)"}
            >
              {isPlaying ? (
                <Pause className="w-4 h-4 fill-current text-primary" />
              ) : (
                <Play className="w-4 h-4 fill-current text-white" />
              )}
            </button>

            <button
              type="button"
              onClick={() => jumpRelative(-5)}
              className="p-1.5 rounded-md hover:bg-white/10 text-stone-300 hover:text-white transition-colors"
              aria-label="Seek back 5 seconds (Left Arrow)"
              title="Seek back 5s"
            >
              <RotateCcw className="w-3.5 h-3.5" />
            </button>

            <button
              type="button"
              onClick={() => jumpRelative(5)}
              className="p-1.5 rounded-md hover:bg-white/10 text-stone-300 hover:text-white transition-colors"
              aria-label="Seek forward 5 seconds (Right Arrow)"
              title="Seek forward 5s"
            >
              <RotateCw className="w-3.5 h-3.5" />
            </button>

            {/* Tabular Monospace Elapsed / Total Time */}
            <div className="font-mono text-[11px] tabular-nums text-stone-300 tracking-tight ml-1">
              <span className="text-white font-medium">{formatTime(currentTime)}</span>
              <span className="text-stone-500 mx-1">/</span>
              <span>{formatTime(durationSeconds)}</span>
            </div>
          </div>

          {/* Right Controls: Speed Selector, Volume Slider, Fullscreen */}
          <div className="flex items-center gap-2.5">
            {/* Speed Selector Dropdown */}
            <div className="relative">
              <button
                type="button"
                onClick={() => setShowSpeedMenu(!showSpeedMenu)}
                className="px-2 py-0.5 rounded bg-white/10 hover:bg-white/20 font-mono text-[11px] text-stone-200 font-medium transition-colors"
                aria-label="Playback speed"
                aria-expanded={showSpeedMenu}
              >
                {playbackSpeed}x
              </button>

              {showSpeedMenu && (
                <div
                  role="menu"
                  aria-label="Select playback speed"
                  className="absolute bottom-8 right-0 py-1 bg-surface-elevated text-ink rounded shadow-lg border border-border text-xs z-50 min-w-[70px]"
                >
                  {PLAYBACK_SPEEDS.map((spd) => (
                    <button
                      key={spd}
                      type="button"
                      role="menuitem"
                      onClick={() => handleSpeedChange(spd)}
                      className={`w-full px-3 py-1 text-left font-mono text-[11px] hover:bg-surface flex items-center justify-between ${
                        playbackSpeed === spd ? "text-primary font-bold" : ""
                      }`}
                    >
                      <span>{spd}x</span>
                      {playbackSpeed === spd && <span className="w-1.5 h-1.5 rounded-full bg-primary" />}
                    </button>
                  ))}
                </div>
              )}
            </div>

            {/* Volume Control */}
            <div className="flex items-center gap-1.5 group">
              <button
                type="button"
                onClick={toggleMute}
                className="p-1 rounded text-stone-300 hover:text-white transition-colors"
                aria-label={isMuted ? "Unmute audio" : "Mute audio"}
              >
                {isMuted || volume === 0 ? (
                  <VolumeX className="w-3.5 h-3.5 text-status-warning" />
                ) : (
                  <Volume2 className="w-3.5 h-3.5" />
                )}
              </button>

              <input
                type="range"
                min="0"
                max="1"
                step="0.05"
                value={isMuted ? 0 : volume}
                onChange={(e) => handleVolumeChange(parseFloat(e.target.value))}
                className="w-16 h-1 accent-primary cursor-pointer hidden sm:inline-block"
                aria-label="Adjust volume"
              />
            </div>

            {/* Fullscreen Toggle (only for video) */}
            {hasVideo && (
              <button
                type="button"
                onClick={toggleFullscreen}
                className="p-1 rounded text-stone-300 hover:text-white transition-colors"
                aria-label={isFullscreen ? "Exit fullscreen" : "Enter fullscreen"}
              >
                {isFullscreen ? <Minimize className="w-3.5 h-3.5" /> : <Maximize className="w-3.5 h-3.5" />}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
};
