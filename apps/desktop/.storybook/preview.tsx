import type { Preview } from "@storybook/react-vite";
import "../src/styles/index.css";
import "../src/styles/features/settings-material-unified.css";

const preview: Preview = {
  globalTypes: {
    theme: {
      description: "界面主题",
      toolbar: {
        icon: "circlehollow",
        items: [
          { value: "light", title: "亮色" },
          { value: "dark", title: "暗色" },
        ],
      },
    },
  },
  initialGlobals: {
    theme: "light",
  },
  decorators: [
    (Story, context) => {
      const theme = context.globals.theme === "dark" ? "dark" : "light";
      document.documentElement.dataset.theme = theme;
      document.documentElement.dataset.tone = "twilight";
      document.documentElement.style.colorScheme = theme;
      return <Story />;
    },
  ],
  parameters: {
    layout: "fullscreen",
    controls: { disableSaveFromUI: true },
  },
};

export default preview;
