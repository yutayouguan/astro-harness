/** Anthropic citations 引用折叠块，带手风琴展开动画。 */
import { memo, useState } from "react";
import { BookOpen } from "lucide-react";

type Citation = Record<string, unknown>;

type Props = {
  citations: Citation[];
};

function MsgCitationsImpl({ citations }: Props) {
  const [open, setOpen] = useState(false);

  if (!citations.length) return null;

  return (
    <div className={`msg-citations ${open ? "is-open" : ""}`}>
      <button
        type="button"
        className="msg-citations-toggle"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <BookOpen
          size={14}
          strokeWidth={1.75}
          className="msg-citations-icon"
          aria-hidden
        />
        <span className="msg-citations-label">
          {citations.length} {citations.length === 1 ? "citation" : "citations"}
        </span>
      </button>
      <div className="msg-citations-collapse">
        <ul className="msg-citations-list">
          {citations.map((c, i) => (
            <li key={i} className="msg-citations-item">
              {typeof c.cited_text === "string" && (
                <blockquote className="msg-citations-quote">
                  {c.cited_text}
                </blockquote>
              )}
              {typeof c.document_title === "string" && (
                <span className="msg-citations-source">{c.document_title}</span>
              )}
              {!c.cited_text && !c.document_title && (
                <span className="msg-citations-raw">{JSON.stringify(c)}</span>
              )}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

// 流式 flush 只替换当前消息对象，历史引用块保持引用不变，避免整棵聊天树重渲染。
export default memo(MsgCitationsImpl);
