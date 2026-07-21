/** 推理强度选择器 —— AI 节点共用 */

import { SelectField } from "./ConfigField";

interface Props {
  value: string;
  onChange: (v: string) => void;
}

export default function ReasoningLevelSelect({ value, onChange }: Props) {
  return (
    <SelectField
      label="推理强度"
      value={value || "medium"}
      onChange={onChange}
      options={[
        { value: "auto", label: "自动" },
        { value: "off", label: "关闭" },
        { value: "low", label: "低" },
        { value: "medium", label: "中" },
        { value: "high", label: "高" },
        { value: "xhigh", label: "超高" },
      ]}
    />
  );
}
