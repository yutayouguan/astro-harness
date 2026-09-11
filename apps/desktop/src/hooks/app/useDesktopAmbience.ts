import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { useDesktopPetState } from "./useDesktopPetState";
import type { WallpaperController } from "./useWallpaper";
import type { useActiveUiStyle } from "./useActiveUiStyle";
import type { ShellColorPrefs } from "../../lib/ui/shellGradient";
import type { PetScene } from "../../lib/ui/petScene";
import type { DesktopPetState } from "../../lib/ui/desktopPetState";
import type { ActiveUiStyle } from "../../lib/ui/activeUiStyle";
import {
  addRecentWallpaper,
  resolveExtractedWallpaperPalette,
  type WallpaperAsset,
  type WallpaperPrefs,
} from "../../lib/ui/wallpaper";
import {
  ambiencePreferenceKey,
  materializeWallpaper,
  paletteTokens,
} from "../../lib/ui/desktopAmbience";
import {
  createDynamicSeed,
  dynamicGradientForTab,
} from "../../lib/ui/dynamicGradient";

export type AmbienceProps = {
  wallpaper: WallpaperController;
  colors: ShellColorPrefs;
  restoreColors: (prefs: ShellColorPrefs) => void;
  activeStyle: ReturnType<typeof useActiveUiStyle>;
  theme: "light" | "dark";
  onManage: (target: "scenes" | "wallpapers") => void;
};
type Undo = {
  token: string;
  wallpaper: WallpaperPrefs;
  colors: ShellColorPrefs;
  expected: string;
};
type Change =
  | { kind: "scene"; sceneId: string }
  | { kind: "wallpaper"; path: string | null }
  | {
      kind: "palette";
      adaptiveColor: boolean;
      colors: [string, string] | null;
      darkColors: [string, string] | null;
    };

export function useDesktopAmbience(props: AmbienceProps, open: boolean) {
  const pet = useDesktopPetState(open);
  const [scenes, setScenes] = useState<PetScene[]>([]);
  const [loadingScenes, setLoadingScenes] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [undo, setUndo] = useState<Undo | null>(null);
  const locked = useRef(false);
  const live = useRef(props);
  live.current = props;
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  useEffect(() => {
    if (!open) return;
    let disposed = false;
    setLoadingScenes(true);
    void invoke<PetScene[]>("get_pet_scenes")
      .then((next) => {
        if (!disposed) setScenes(next);
      })
      .catch((cause) => {
        if (!disposed) setError(String(cause));
      })
      .finally(() => {
        if (!disposed) setLoadingScenes(false);
      });
    return () => {
      disposed = true;
    };
  }, [open, pet.state.revision]);

  const key = (p: AmbienceProps) =>
    `${ambiencePreferenceKey(p.wallpaper.prefs)}|${JSON.stringify(p.colors)}`;
  const guard = async (operation: () => Promise<void>) => {
    if (locked.current) return false;
    locked.current = true;
    setBusy(true);
    setError("");
    try {
      await operation();
      return true;
    } catch (cause) {
      if (mounted.current) setError(String(cause));
      return false;
    } finally {
      locked.current = false;
      if (mounted.current) setBusy(false);
    }
  };

  const change = async (
    request: Change,
    edit?: (
      prefs: WallpaperPrefs,
      colors: ShellColorPrefs,
    ) => [WallpaperPrefs, ShellColorPrefs],
  ) => {
    const p = live.current;
    const before = {
      wallpaper: structuredClone(p.wallpaper.prefs),
      colors: structuredClone(p.colors),
    };
    await p.wallpaper.withSuspendedSync(async () => {
      const result = await invoke<{
        state: DesktopPetState;
        undoToken: string;
      }>("apply_desktop_ambience", { change: request });
      pet.accept(result.state);
      let next = materializeWallpaper(
        before.wallpaper,
        p.activeStyle.style,
        p.theme,
      );
      let colors = before.colors;
      if (request.kind === "scene") {
        try {
          const style = await invoke<ActiveUiStyle | null>(
            "get_active_ui_style",
          );
          if (!style?.wallpaper || style.id !== request.sceneId)
            throw new Error("场景壁纸未能读取，已尝试恢复原外观");
          next = materializeWallpaper(before.wallpaper, style, p.theme);
          if (!next.adaptiveColor) colors = { ...colors, style: "unified" };
        } catch (cause) {
          // Do not leave a committed pet switch paired with stale local wallpaper preferences.
          const restored = await invoke<DesktopPetState>(
            "undo_desktop_ambience",
            { token: result.undoToken },
          );
          pet.accept(restored);
          setUndo(null);
          await p.activeStyle.refresh();
          throw cause;
        }
      }
      if (edit) [next, colors] = edit(next, colors);
      p.wallpaper.replacePrefs(next);
      p.restoreColors(colors);
      setUndo({
        ...before,
        token: result.undoToken,
        expected: `${ambiencePreferenceKey(next)}|${JSON.stringify(colors)}`,
      });
      await p.activeStyle.refresh();
      // Refresh the list immediately too; native events can be delivered before the command returns.
      try {
        const updated = await invoke<PetScene[]>("get_pet_scenes");
        if (mounted.current) setScenes(updated);
      } catch (cause) {
        if (mounted.current)
          setError(`已切换，但场景列表刷新失败：${String(cause)}`);
      }
    });
  };

  const selectScene = (sceneId: string) => {
    if (
      pet.state.activeSceneId === sceneId &&
      live.current.activeStyle.style?.id === sceneId
    )
      return Promise.resolve(true);
    return guard(() => change({ kind: "scene", sceneId }));
  };
  const selectWallpaper = (asset: WallpaperAsset, linked: boolean) =>
    guard(async () => {
      const scene = linked
        ? scenes.find((s) => s.wallpaperPath === asset.path)
        : undefined;
      if (scene) await change({ kind: "scene", sceneId: scene.id });
      else
        await change(
          { kind: "wallpaper", path: asset.path },
          (prefs, colors) => [addRecentWallpaper(prefs, asset), colors],
        );
    });
  const followSystem = () =>
    guard(async () => {
      const asset = await invoke<WallpaperAsset>("get_system_wallpaper");
      await change({ kind: "wallpaper", path: asset.path }, (prefs, colors) => [
        { ...addRecentWallpaper(prefs, asset), followSystemWallpaper: true },
        colors,
      ]);
    });
  const clearWallpaper = () =>
    guard(() =>
      change({ kind: "wallpaper", path: null }, (prefs, colors) => [
        { ...prefs, mode: "color", followSystemWallpaper: false },
        colors,
      ]),
    );
  const setPalette = (
    mode: "dynamic" | "wallpaper" | "custom",
    primary?: string,
    secondary?: string,
  ) =>
    guard(async () => {
      if (mode === "custom") paletteTokens(primary ?? "", secondary ?? "");
      const seed = createDynamicSeed();
      const gradient = dynamicGradientForTab(seed, "chat", live.current.theme);
      const paletteFor = (theme: "light" | "dark"): [string, string] => {
        const g = dynamicGradientForTab(seed, "chat", theme);
        const palette = resolveExtractedWallpaperPalette(
          { accentColor: g.primary.color, secondaryColor: g.secondary.color },
          theme,
        )!;
        return [palette.themeColor, palette.highlightColor];
      };
      await change(
        {
          kind: "palette",
          adaptiveColor: mode === "wallpaper",
          colors:
            mode === "wallpaper"
              ? null
              : mode === "dynamic"
                ? paletteFor("light")
                : [primary!, secondary!],
          darkColors:
            mode === "wallpaper"
              ? null
              : mode === "dynamic"
                ? paletteFor("dark")
                : [primary!, secondary!],
        },
        (prefs, colors) => {
          if (mode === "wallpaper")
            return [{ ...prefs, adaptiveColor: true }, colors];
          if (mode === "dynamic") {
            return [
              {
                ...prefs,
                adaptiveColor: false,
                customThemeColor: gradient.primary.color,
                customHighlightColor: gradient.secondary.color,
              },
              { ...colors, style: "dynamic", dynamicSeed: seed },
            ];
          }
          return [
            {
              ...prefs,
              adaptiveColor: false,
              customThemeColor: primary,
              customHighlightColor: secondary,
            },
            {
              ...colors,
              style: "unified",
              gradient: {
                ...colors.gradient,
                id: "custom",
                primary: { ...colors.gradient.primary, color: primary! },
                secondary: { ...colors.gradient.secondary, color: secondary! },
              },
            },
          ];
        },
      );
    });
  const undoLast = () =>
    guard(async () => {
      if (!undo) return;
      const p = live.current;
      if (key(p) !== undo.expected)
        throw new Error("外观已在其他位置修改，无法撤销这一步");
      await p.wallpaper.withSuspendedSync(async () => {
        const restored = await invoke<DesktopPetState>(
          "undo_desktop_ambience",
          { token: undo.token },
        );
        pet.accept(restored);
        p.wallpaper.replacePrefs(undo.wallpaper);
        p.restoreColors(undo.colors);
        setUndo(null);
        await p.activeStyle.refresh();
      });
    });
  const saveAs = (name: string) =>
    guard(async () => {
      const p = live.current;
      const prefs = materializeWallpaper(
        p.wallpaper.prefs,
        p.activeStyle.style,
        p.theme,
      );
      if (
        !prefs.current ||
        prefs.mode !== "wallpaper" ||
        !pet.state.activePetId
      )
        throw new Error("请先选择宠物和壁纸");
      await pet.mutate("save_desktop_ambience_scene", {
        name: name.trim(),
        petId: pet.state.activePetId,
        wallpaper: {
          path: prefs.current.path,
          fit: prefs.fit,
          shade: prefs.shade,
          blur: prefs.blur,
          adaptiveColor: prefs.adaptiveColor,
        },
        tokens: prefs.adaptiveColor
          ? { light: {}, dark: {} }
          : paletteTokens(
              prefs.customThemeColor ?? "#4f6ef7",
              prefs.customHighlightColor ?? "#22b8a7",
            ),
      });
    });
  return {
    pet: pet.state,
    scenes,
    busy: busy || pet.loading || loadingScenes,
    error: error || pet.error,
    canUndo: Boolean(undo),
    selectScene,
    selectWallpaper,
    followSystem,
    clearWallpaper,
    setPalette,
    undoLast,
    saveAs,
  };
}
