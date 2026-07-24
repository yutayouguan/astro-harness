/** AI 助手抽屉 — 对话式生成/修改工作流 */

import { useCallback, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Sparkles,
  Send,
  X,
  Loader2,
  CheckCircle2,
  AlertCircle,
  Wand2,
} from "lucide-react";
import { NODE_REGISTRY } from "./loopTypes";

interface AiGenNode {
  id: string;
  node_type: string;
  label: string;
  config: Record<string, unknown>;
}

interface AiGenEdge {
  source: string;
  target: string;
  source_handle?: string | null;
}

interface AiResult {
  explanation: string;
  nodes: AiGenNode[];
  edges: AiGenEdge[];
}

interface ChatMessage {
  role: "user" | "assistant" | "error";
  content: string;
  result?: AiResult;
}

interface Props {
  currentNodes: { id: string; node_type: string; label: string; config: Record<string, unknown> }[];
  onApply: (nodes: AiGenNode[], edges: AiGenEdge[]) => void;
  onClose: () => void;
}

export default function LoopAiAssistant({ currentNodes, onApply, onClose }: Props) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const scrollToBottom = useCallback(() => {
    setTimeout(() => messagesEndRef.current?.scrollIntoView({ behavior: "smooth" }), 50);
  }, []);

  const handleSend = async () => {
    const text = input.trim();
    if (!text || loading) return;

    setInput("");
    setMessages((prev) => [...prev, { role: "user", content: text }]);
    setLoading(true);
    scrollToBottom();

    try {
      const result = await invoke<AiResult>("ai_generate_workflow", {
        prompt: text,
        currentNodes: currentNodes.length > 0 ? currentNodes : null,
      });

      setMessages((prev) => [
        ...prev,
        {
          role: "assistant",
          content: result.explanation,
          result,
        },
      ]);
    } catch (e) {
      setMessages((prev) => [
        ...prev,
        { role: "error", content: String(e) },
      ]);
    } finally {
      setLoading(false);
      scrollToBottom();
    }
  };

  const handleApply = (result: AiResult) => {
    onApply(result.nodes, result.edges);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void handleSend();
    }
  };

  return (
    <div className="loop-ai-assistant">
      {/* Header */}
      <div className="loop-ai-header">
        <Sparkles size={16} />
        <span>AI 助手</span>
        <button className="loop-icon-btn" onClick={onClose} style={{ marginLeft: "auto" }}>
          <X size={14} />
        </button>
      </div>

      {/* Messages */}
      <div className="loop-ai-messages">
        {messages.length === 0 && (
          <div className="loop-ai-welcome">
            <div className="loop-ai-welcome-icon">
              <Wand2 size={24} />
            </div>
            <p className="loop-ai-welcome-title">描述你想创建的工作流</p>
            <p className="loop-ai-welcome-hint">
              例如：「每天早上9点抓取RSS新闻，用AI总结后发到Webhook」
            </p>
            <div className="loop-ai-suggestions">
              {[
                "创建一个定时RSS抓取 → AI总结 → Webhook推送的流程",
                "帮我搭一个用户提问 → 问题分类 → 分别处理的流程",
                "创建一个图片生成 → 配音 → 合成视频的流程",
              ].map((s) => (
                <button
                  key={s}
                  className="loop-ai-suggestion"
                  onClick={() => {
                    setInput(s);
                    inputRef.current?.focus();
                  }}
                >
                  {s}
                </button>
              ))}
            </div>
          </div>
        )}

        {messages.map((msg, i) => (
          <div key={i} className={`loop-ai-msg loop-ai-msg--${msg.role}`}>
            {msg.role === "user" && (
              <div className="loop-ai-msg-bubble loop-ai-msg-bubble--user">
                {msg.content}
              </div>
            )}
            {msg.role === "assistant" && (
              <div className="loop-ai-msg-bubble loop-ai-msg-bubble--assistant">
                <p>{msg.content}</p>
                {msg.result && (
                  <div className="loop-ai-result-summary">
                    <div className="loop-ai-result-stats">
                      <CheckCircle2 size={14} />
                      <span>{msg.result.nodes.length} 个节点</span>
                      <span>·</span>
                      <span>{msg.result.edges.length} 条连线</span>
                    </div>
                    <div className="loop-ai-result-nodes">
                      {msg.result.nodes.map((n) => {
                        const meta = NODE_REGISTRY.find((m) => m.type === n.node_type);
                        return (
                          <span key={n.id} className="loop-ai-result-node-tag">
                            {meta?.label ?? n.node_type}
                          </span>
                        );
                      })}
                    </div>
                    <button
                      className="loop-btn loop-btn--primary loop-ai-apply-btn"
                      onClick={() => handleApply(msg.result!)}
                    >
                      <Sparkles size={13} />
                      <span>应用到画布</span>
                    </button>
                  </div>
                )}
              </div>
            )}
            {msg.role === "error" && (
              <div className="loop-ai-msg-bubble loop-ai-msg-bubble--error">
                <AlertCircle size={14} />
                <span>{msg.content}</span>
              </div>
            )}
          </div>
        ))}

        {loading && (
          <div className="loop-ai-msg loop-ai-msg--assistant">
            <div className="loop-ai-msg-bubble loop-ai-msg-bubble--loading">
              <Loader2 size={16} className="loop-ai-spinner" />
              <span>正在设计工作流…</span>
            </div>
          </div>
        )}

        <div ref={messagesEndRef} />
      </div>

      {/* Input */}
      <div className="loop-ai-input-area">
        <textarea
          ref={inputRef}
          className="loop-ai-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="描述你想要的工作流…"
          rows={2}
          disabled={loading}
        />
        <button
          className="loop-ai-send-btn"
          onClick={() => void handleSend()}
          disabled={!input.trim() || loading}
        >
          <Send size={16} />
        </button>
      </div>
    </div>
  );
}
