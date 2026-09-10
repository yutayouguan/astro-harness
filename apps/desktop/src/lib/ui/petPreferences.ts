export type PetPosition = {
  monitor: string | null;
  monitorX: number;
  monitorY: number;
  x: number;
  y: number;
};
export type PetPreferences = {
  position: PetPosition | null;
  positionLocked: boolean;
  snapToEdge: boolean;
  quietMode: boolean;
  hideInFullscreen: boolean;
  presentationMode: boolean;
  activityIntervalSecs: number;
};
export const DEFAULT_PET_PREFERENCES: PetPreferences = {
  position: null,
  positionLocked: false,
  snapToEdge: true,
  quietMode: false,
  hideInFullscreen: true,
  presentationMode: false,
  activityIntervalSecs: 45,
};
