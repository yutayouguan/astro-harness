/** 毛玻璃风格音频播放器，替代原生黑色 controls。 */
import { useEffect, useRef, useState, type CSSProperties } from "react";
import {
  Pause as PauseData,
  Play as PlayData,
  Volume2 as Volume2Data,
  VolumeX as VolumeXData,
} from "lucide";
import { MorphToggleIcon } from "../icons/MorphIcon";
import { useI18n } from "../../i18n/LocaleContext";

const PAUSE_ICON = PauseData;
const PLAY_ICON = PlayData;
const VOLUME_ON_ICON = Volume2Data;
const VOLUME_OFF_ICON = VolumeXData;

type Props = {
  src: string;
  className?: string;
  onError?: () => void;
};

function formatTime(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return "00:00";
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

export default function GlassAudioPlayer({ src, className, onError }: Props) {
  const { t } = useI18n();
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const [playing, setPlaying] = useState(false);
  const [current, setCurrent] = useState(0);
  const [duration, setDuration] = useState(0);
  const [muted, setMuted] = useState(false);
  const [seeking, setSeeking] = useState(false);

  useEffect(() => {
    const el = audioRef.current;
    if (!el) return;
    el.pause();
    el.currentTime = 0;
    setPlaying(false);
    setCurrent(0);
    setDuration(0);
  }, [src]);

  const togglePlay = () => {
    const el = audioRef.current;
    if (!el) return;
    if (el.paused) {
      void el
        .play()
        .then(() => setPlaying(true))
        .catch(() => setPlaying(false));
    } else {
      el.pause();
      setPlaying(false);
    }
  };

  const onSeek = (value: number) => {
    const el = audioRef.current;
    if (!el) return;
    el.currentTime = value;
    setCurrent(value);
  };

  const progress = duration > 0 ? Math.min(100, (current / duration) * 100) : 0;

  return (
    <div className={`glass-audio ${className ?? ""}`.trim()}>
      <audio
        ref={audioRef}
        src={src}
        preload="metadata"
        muted={muted}
        onError={() => onError?.()}
        onLoadedMetadata={() => {
          const el = audioRef.current;
          if (el) setDuration(el.duration || 0);
        }}
        onTimeUpdate={() => {
          if (seeking) return;
          const el = audioRef.current;
          if (el) setCurrent(el.currentTime || 0);
        }}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => {
          setPlaying(false);
          setCurrent(0);
        }}
      />
      <button
        type="button"
        className="glass-audio-play"
        aria-label={playing ? t("media.pause") : t("media.play")}
        onClick={togglePlay}
      >
        <MorphToggleIcon
          active={playing}
          activeIcon={PAUSE_ICON}
          inactiveIcon={PLAY_ICON}
          size={16}
          strokeWidth={2.25}
          aria-hidden
        />
      </button>
      <span className="glass-audio-time">{formatTime(current)}</span>
      <label className="glass-audio-seek">
        <span className="sr-only">{t("media.seek")}</span>
        <input
          type="range"
          min={0}
          max={duration || 0}
          step={0.1}
          value={Math.min(current, duration || 0)}
          disabled={!duration}
          style={{ "--glass-audio-progress": `${progress}%` } as CSSProperties}
          onPointerDown={() => setSeeking(true)}
          onPointerUp={(e) => {
            setSeeking(false);
            onSeek(Number(e.currentTarget.value));
          }}
          onChange={(e) => {
            const next = Number(e.currentTarget.value);
            setCurrent(next);
            if (!seeking) onSeek(next);
          }}
        />
      </label>
      <span className="glass-audio-time">{formatTime(duration)}</span>
      <button
        type="button"
        className="glass-audio-mute"
        aria-label={muted ? t("media.unmute") : t("media.mute")}
        onClick={() => setMuted((v) => !v)}
      >
        <MorphToggleIcon
          active={muted}
          activeIcon={VOLUME_OFF_ICON}
          inactiveIcon={VOLUME_ON_ICON}
          size={15}
          strokeWidth={2.1}
          aria-hidden
        />
      </button>
    </div>
  );
}
