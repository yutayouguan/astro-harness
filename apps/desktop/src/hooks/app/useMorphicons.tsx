import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  readMorphiconPrefs,
  writeMorphiconPrefs,
  type MorphiconPrefs,
  type MorphiconSpring,
  type MorphiconStrokeWidth,
} from "../../lib/ui/morphiconPrefs";

type MorphiconContextValue = MorphiconPrefs & {
  setSpring: (spring: MorphiconSpring) => void;
  setStrokeWidth: (strokeWidth: MorphiconStrokeWidth) => void;
};

const MorphiconContext = createContext<MorphiconContextValue | null>(null);

export function MorphiconProvider({ children }: { children: ReactNode }) {
  const [prefs, setPrefs] = useState<MorphiconPrefs>(() =>
    typeof window === "undefined"
      ? { spring: "smooth", strokeWidth: 2 }
      : readMorphiconPrefs(),
  );

  useEffect(() => {
    writeMorphiconPrefs(prefs);
  }, [prefs]);

  useLayoutEffect(() => {
    if (typeof document === "undefined") return;
    const root = document.documentElement;
    root.dataset.iconMotion = prefs.spring;
    root.style.setProperty(
      "--app-icon-stroke-width",
      String(prefs.strokeWidth),
    );
  }, [prefs]);

  useEffect(
    () => () => {
      if (typeof document === "undefined") return;
      delete document.documentElement.dataset.iconMotion;
      document.documentElement.style.removeProperty("--app-icon-stroke-width");
    },
    [],
  );

  const setSpring = useCallback((spring: MorphiconSpring) => {
    setPrefs((current) => ({ ...current, spring }));
  }, []);

  const setStrokeWidth = useCallback((strokeWidth: MorphiconStrokeWidth) => {
    setPrefs((current) => ({ ...current, strokeWidth }));
  }, []);

  const value = useMemo(
    () => ({ ...prefs, setSpring, setStrokeWidth }),
    [prefs, setSpring, setStrokeWidth],
  );

  return (
    <MorphiconContext.Provider value={value}>
      {children}
    </MorphiconContext.Provider>
  );
}

export function useMorphicons(): MorphiconContextValue {
  const context = useContext(MorphiconContext);
  if (!context)
    throw new Error("useMorphicons must be used within MorphiconProvider");
  return context;
}
