import App from "../App";
import { StrictMode } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import OnboardingGate from "../components/onboarding/OnboardingGate";
import { ThemeProvider } from "../hooks/app/useTheme";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { LocaleProvider } from "../i18n/LocaleContext";
import { ActiveAgentProvider } from "../hooks/app/useActiveAgent";
import { DialogProvider } from "../hooks/ui/DialogContext";

/** Browser tests inject only the native transport; this renders the production gate and App. */
function Runtime() {
  return (
    <ThemeProvider>
      <MorphiconProvider>
        <LocaleProvider>
          <OnboardingGate>
            <ActiveAgentProvider>
              <DialogProvider>
                <App />
              </DialogProvider>
            </ActiveAgentProvider>
          </OnboardingGate>
        </LocaleProvider>
      </MorphiconProvider>
    </ThemeProvider>
  );
}
const meta = {
  title: "App/Onboarding runtime",
  component: Runtime,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof Runtime>;
export default meta;
type Story = StoryObj<typeof meta>;
export const NativeTransport: Story = {};
export const StrictNativeTransport: Story = {
  render: () => (
    <StrictMode>
      <Runtime />
    </StrictMode>
  ),
};
