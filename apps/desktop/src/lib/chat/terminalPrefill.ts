/**
 * 把一个命令预填进新开的终端（不执行）。
 *
 * 审批卡里的「在终端打开」用它把沙箱拒绝的命令交给用户自己跑：终端只是预填，
 * 用户核对后自己回车。终端 dock 由 App 拥有，所以这里走进程内事件而不是 props。
 */
export const TERMINAL_PREFILL_EVENT = "astro:terminal-prefill";

export type TerminalPrefillRequest = {
  /** 每次请求自增：重复点击同一条命令也要再开一次。 */
  token: number;
  command: string;
};

let prefillToken = 0;

export function openCommandInTerminal(command: string): boolean {
  const trimmed = command.trim();
  if (!trimmed) return false;
  if (typeof window === "undefined") return false;
  prefillToken += 1;
  window.dispatchEvent(
    new CustomEvent<TerminalPrefillRequest>(TERMINAL_PREFILL_EVENT, {
      detail: { token: prefillToken, command: trimmed },
    }),
  );
  return true;
}
