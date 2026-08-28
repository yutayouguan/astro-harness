import type { Meta, StoryObj } from "@storybook/react-vite";
import { ChatWelcome } from "../components/chat/ChatWelcome";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { LocaleProvider } from "../i18n/LocaleContext";

function ChatWelcomePreview() {
  return (
    <LocaleProvider>
      <MorphiconProvider>
        <main
          style={{
            width: "100vw",
            height: "100vh",
            minHeight: 0,
            display: "flex",
            flexDirection: "column",
            overflow: "hidden",
            background: "var(--shell-bg)",
            color: "var(--ink)",
          }}
        >
          <ChatWelcome onPickCard={() => {}} />
        </main>
      </MorphiconProvider>
    </LocaleProvider>
  );
}

const meta = {
  id: "chat-welcome-marquee",
  title: "Chat/Welcome Marquee",
  component: ChatWelcomePreview,
  parameters: {
    controls: { disable: true },
  },
} satisfies Meta<typeof ChatWelcomePreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
