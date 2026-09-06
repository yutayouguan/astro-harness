import type { UiSurface } from "../../types.ts";

export function isLocationRequiredSurface(surface: UiSurface): boolean {
  return (
    surface.status === "active" &&
    Boolean(
      surface.interrupts?.some((item) => item.reason === "location_required"),
    )
  );
}
