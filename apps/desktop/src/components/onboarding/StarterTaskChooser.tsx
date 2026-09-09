import { useState } from "react";
import {
  Eye,
  FileText,
  Languages,
  Lightbulb,
  MessageSquarePlus,
} from "lucide-react";
import { Button } from "../ui";
import {
  buildStarterPrompt,
  STARTER_TASKS,
  STARTER_INPUT_LIMIT,
  type StarterTask,
} from "../../lib/ui/onboardingTasks";

export function StarterTaskChooser({
  locale,
  onUse,
}: {
  locale: "zh" | "en";
  onUse: (prompt: string) => void;
}) {
  const [task, setTask] = useState<StarterTask>("summarize");
  const [text, setText] = useState("");
  const zh = locale === "zh";
  const titles = zh
    ? ["整理一段文字", "翻译一小段", "解释一段内容"]
    : ["Summarize text", "Translate a passage", "Explain a snippet"];
  const icons = [FileText, Languages, Lightbulb];
  const prompt = buildStarterPrompt(task, text, locale);
  return (
    <section
      className="onboarding-starters"
      aria-label={zh ? "30 秒小任务" : "A small first task"}
    >
      <h2>
        {zh ? "从一个 30 秒小任务开始" : "Start with a small, 30-second task"}
      </h2>
      <div className="onboarding-task-tabs">
        {STARTER_TASKS.map((value, index) => {
          const Icon = icons[index];
          return (
            <button
              key={value}
              type="button"
              aria-pressed={task === value}
              onClick={() => setTask(value)}
            >
              <Icon size={16} aria-hidden />
              <span>{titles[index]}</span>
            </button>
          );
        })}
      </div>
      <label className="onboarding-task-input">
        <span>
          {zh ? "填入你想处理的内容" : "Paste the content to work with"}
        </span>
        <textarea
          value={text}
          maxLength={STARTER_INPUT_LIMIT}
          rows={3}
          onChange={(event) => setText(event.target.value)}
          placeholder={
            zh
              ? "粘贴一小段文字或代码，不需要选择文件或开放目录权限。"
              : "Paste a short passage or snippet. No file or folder access is needed."
          }
        />
      </label>
      <details className="onboarding-task-preview" open>
        <summary>
          <span className="onboarding-inline-label">
            <Eye size={14} aria-hidden />
            {zh ? "发送内容预览" : "Prompt preview"}
          </span>
        </summary>
        <pre>
          {prompt ??
            (zh
              ? "填写内容后，这里会显示完整提示词。"
              : "Enter content to preview the complete prompt.")}
        </pre>
      </details>
      <p className="onboarding-task-privacy">
        {zh
          ? "这里只预填草稿，不会自动发送。点击聊天中的发送后，内容才会交给你配置的模型；请勿粘贴敏感信息。"
          : "This only prepares a draft. Content reaches your configured model only after you press Send in chat. Avoid sensitive information."}
      </p>
      <Button
        variant="primary"
        disabled={!prompt}
        onClick={() => {
          if (prompt) onUse(prompt);
        }}
      >
        <MessageSquarePlus size={16} aria-hidden />
        {zh ? "填入新对话" : "Prepare a new chat"}
      </Button>
    </section>
  );
}
