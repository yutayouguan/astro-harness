export type StarterTask = "summarize" | "translate" | "explain";
export const STARTER_TASKS: StarterTask[] = [
  "summarize",
  "translate",
  "explain",
];
export const STARTER_INPUT_LIMIT = 4000;
export function buildStarterPrompt(
  task: StarterTask,
  text: string,
  locale: "zh" | "en",
): string | null {
  const body = text.trim();
  if (!body || body.length > STARTER_INPUT_LIMIT) return null;
  const requests = {
    zh: {
      summarize:
        "请把下面的文字整理成三个简明要点。只处理提供的内容，不调用工具或读取文件：",
      translate:
        "请把下面的文字翻译为英文；如果原文是英文，则翻译为中文。保留原意，只输出译文，不调用工具：",
      explain:
        "请用简洁中文解释下面的内容，先用一句话概括，再指出两个关键点。只处理提供的内容，不调用工具或读取文件：",
    },
    en: {
      summarize:
        "Summarize the following text in three concise points. Only use the provided content; do not call tools or read files:",
      translate:
        "Translate the following text into English, or into Chinese if it is already English. Preserve its meaning and return only the translation. Do not call tools:",
      explain:
        "Explain the following content in plain English: one sentence followed by two key points. Only use the provided content; do not call tools or read files:",
    },
  };
  return requests[locale][task] + "\n\n" + body;
}
