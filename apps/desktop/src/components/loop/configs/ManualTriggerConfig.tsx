import { Section } from "./ConfigField";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function ManualTriggerConfig(_props: ConfigProps) {
  return (
    <Section title="手动触发">
      <p className="loop-config-hint">
        手动触发无需额外配置。点击「运行一次」即可启动。
      </p>
    </Section>
  );
}
