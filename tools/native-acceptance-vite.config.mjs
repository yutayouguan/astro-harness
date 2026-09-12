import config from "../apps/desktop/vite.config.ts";

// Native QA shares a checkout with other work. A hot reload must not erase
// half-entered credentials or restart the interaction under test.
export default {
  ...config,
  server: { ...config.server, watch: null, hmr: false },
};
