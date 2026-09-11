import type { Meta, StoryObj } from "@storybook/react-vite";
import { useTheme } from "../hooks/app/useTheme";
import softMeta from "./SoftMaterial.stories";
import { SelectMenu } from "../components/ui/SelectMenu";
import { useState, type CSSProperties } from "react";

/** Uses the production editor structure/classes, without sending real messages. */
function ComposerFocusSample() {
  const { setMode, material, setMaterial } = useTheme();
  const [accent, setAccent] = useState("#155eef");
  return (
    <div
      style={
        { height: "100%", overflow: "auto", "--tone": accent } as CSSProperties
      }
    >
      <div style={{ display: "flex", gap: 12 }}>
        <button type="button" onClick={() => setMode("light")}>
          亮色
        </button>
        <button type="button" onClick={() => setMode("dark")}>
          暗色
        </button>
        <button
          type="button"
          onClick={() => setMaterial(material === "soft" ? "glass" : "soft")}
        >
          材质：{material}
        </button>
        <button type="button" onClick={() => setAccent("#155eef")}>
          蓝色强调
        </button>
        <button type="button" onClick={() => setAccent("#16a34a")}>
          绿色强调
        </button>
        <button type="button" onClick={() => setAccent("#f97316")}>
          橙色强调
        </button>
      </div>
      {(["expanded", "capsule"] as const).map((layout) => (
        <section
          key={layout}
          aria-label={layout}
          style={{ position: "relative", height: 210 }}
        >
          <h2>{layout === "capsule" ? "胶囊输入" : "展开输入"}</h2>
          <form
            className={`composer-shell ${layout === "capsule" ? "is-capsule" : ""}`}
            onSubmit={(event) => event.preventDefault()}
          >
            <div className="composer composer--stacked">
              <div className="composer-input-wrap">
                <textarea
                  className="composer-input"
                  aria-label={`${layout} 输入框`}
                  rows={2}
                  placeholder="点击或用 Tab 聚焦，不应出现内部方框"
                />
              </div>
              <div className="composer-bar">
                <div className="composer-bar-left">
                  <button type="button" className="composer-mode-pill">
                    默认模式
                  </button>
                </div>
                <div className="composer-bar-right">
                  <button
                    type="button"
                    className="send-btn send-btn--round"
                    aria-label={`${layout} 发送`}
                  >
                    ↑
                  </button>
                </div>
              </div>
            </div>
          </form>
        </section>
      ))}
      <section
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(2, minmax(0, 1fr))",
          gap: 20,
          padding: 12,
        }}
      >
        <label className="providers-field">
          普通输入
          <input aria-label="普通输入" />
        </label>
        <label className="providers-field">
          密码
          <input type="password" aria-label="密码" />
        </label>
        <label className="sidebar-settings-search">
          <span style={{ whiteSpace: "nowrap" }}>搜索</span>
          <input type="search" aria-label="搜索" />
        </label>
        <div className="cron-sched-stepper">
          <input
            className="cron-sched-input cron-sched-input--bare"
            type="number"
            aria-label="数字"
            defaultValue={3}
          />
        </div>
        <label>
          独立文本框
          <textarea
            className="user-message-editor-input"
            aria-label="独立文本框"
          />
        </label>
        <label>
          原生选择
          <select className="prefs-select" aria-label="原生选择">
            <option>默认</option>
            <option>备用</option>
          </select>
        </label>
        <SelectMenu
          aria-label="自定义选择"
          value="default"
          options={[
            { value: "default", label: "默认" },
            { value: "other", label: "备用" },
          ]}
          onChange={() => {}}
          search={{ placeholder: "筛选选项", emptyLabel: "没有结果" }}
        />
        <label className="providers-field">
          错误输入
          <input
            aria-label="错误输入"
            aria-invalid="true"
            defaultValue="无效值"
          />
        </label>
        <label className="providers-field">
          禁用输入
          <input aria-label="禁用输入" disabled />
        </label>
        <div className="prefs-page is-embedded">
          <input
            className="aux-number-input"
            type="number"
            aria-label="设置数字"
            defaultValue={1}
          />
        </div>
      </section>
    </div>
  );
}

const meta = {
  title: "Design/Composer Focus",
  component: ComposerFocusSample,
  decorators: softMeta.decorators,
  beforeEach: softMeta.beforeEach,
} satisfies Meta<typeof ComposerFocusSample>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Regression: Story = {};
