import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ExternalLink, X } from "lucide-react";
import { useEffect, useState } from "react";

import type { DesktopPetState } from "../settings/DesktopPetPanel";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";

const EMPTY_STATE: DesktopPetState = {
  enabled: false,
  sourcePath: null,
  petPath: null,
  scale: 1,
  alwaysOnTop: true,
  updatedAt: "",
  provider: null,
  model: null,
};

export default function DesktopPetSurface() {
  const [state, setState] = useState<DesktopPetState>(EMPTY_STATE);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void invoke<DesktopPetState>("get_desktop_pet_state")
      .then((next) => {
        if (!disposed) setState(next);
      })
      .catch(() => {});
    void listen<DesktopPetState>("desktop-pet-changed", (event) => {
      if (!disposed) setState(event.payload);
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const petSrc = state.petPath ? resolveMediaSrc(state.petPath) : "";

  return (
    <main className="desktop-pet-surface">
      <div
        className="desktop-pet-stage"
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          void getCurrentWindow().startDragging();
        }}
        onDoubleClick={() => void invoke("open_desktop_pet_main")}
      >
        {petSrc ? (
          <img
            className="desktop-pet-character"
            src={petSrc}
            alt="Desktop pet"
            draggable={false}
          />
        ) : (
          <div className="desktop-pet-placeholder" aria-label="Desktop pet">
            <span>🐾</span>
          </div>
        )}
        <div className="desktop-pet-actions" data-tauri-drag-region="false">
          <button
            type="button"
            aria-label="Open Astro"
            title="Open Astro"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={() => void invoke("open_desktop_pet_main")}
          >
            <ExternalLink size={14} />
          </button>
          <button
            type="button"
            aria-label="Hide desktop pet"
            title="Hide desktop pet"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={() =>
              void invoke("set_desktop_pet_enabled", { enabled: false })
            }
          >
            <X size={14} />
          </button>
        </div>
      </div>
    </main>
  );
}
