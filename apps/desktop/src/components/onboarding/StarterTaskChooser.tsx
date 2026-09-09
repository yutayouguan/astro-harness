import {
  FileText,
  FolderTree,
  Laptop,
  ListChecks,
  MessageSquarePlus,
  Workflow,
  FolderCode,
} from "lucide-react";
import { STARTER_TASKS } from "../../lib/ui/onboardingTasks";

const TASK_ICONS = {
  desktop: Laptop,
  organize: FolderTree,
  documents: FileText,
  weekly: ListChecks,
  project: FolderCode,
  automation: Workflow,
};

export function StarterTaskChooser({
  locale,
  onUse,
}: {
  locale: "zh" | "en";
  onUse: (prompt: string) => void;
}) {
  const zh = locale === "zh";
  return (
    <section
      className="onboarding-starters"
      aria-label={zh ? "实用任务示例" : "Practical first tasks"}
    >
      <h2>
        {zh
          ? "选一件实用的小事，开始协作"
          : "Pick something useful to start with"}
      </h2>
      <div className="onboarding-task-grid">
        {STARTER_TASKS.map((task) => {
          const Icon = TASK_ICONS[task.id];
          const copy = task[locale];
          return (
            <button
              key={task.id}
              type="button"
              className="onboarding-task-card"
              aria-label={`${copy.title} · ${zh ? "填入输入框" : "Fill chat input"}`}
              onClick={() => onUse(copy.prompt)}
            >
              <Icon size={18} aria-hidden />
              <span>
                <strong>{copy.title}</strong>
                <small>{copy.description}</small>
              </span>
              <MessageSquarePlus size={15} aria-hidden />
            </button>
          );
        })}
      </div>
      <p className="onboarding-task-privacy">
        {zh
          ? "点击示例即可填入聊天输入框，可继续编辑，不会自动发送。发送后才会开始协作；涉及文件的任务会先确认范围，请留意隐私内容。"
          : "Click an example to fill the chat input. Edit it before sending; nothing is sent automatically. File tasks confirm scope first. Be mindful of private information."}
      </p>
    </section>
  );
}
