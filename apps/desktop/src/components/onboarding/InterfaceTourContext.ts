import { createContext } from "react";

// App mounts during the portal handoff; the tour must wait until it finishes.
export const InterfaceTourReady = createContext(false);
