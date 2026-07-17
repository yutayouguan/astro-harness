/**
 * 从（可能不完整的）file_ops 工具参数 JSON 中容错提取 path / operation / content。
 *
 * 流式场景下，模型逐块吐出工具参数，JSON 往往未闭合（content 字符串未结束）。
 * 这里先尝试完整解析，失败则按字段扫描，支持读取「已到达的部分 content」用于实时预览。
 */

export type PartialFileWrite = {
  path?: string;
  operation?: string;
  content?: string;
  /** content 字符串是否已完整闭合（false 表示仍在流式追加中）。 */
  complete: boolean;
};

/** 定位 `"key"\s*:\s*"` 之后（开引号之后）的下标；无则返回 -1。 */
function findStringValueStart(src: string, key: string): number {
  const re = new RegExp(`"${key}"\\s*:\\s*"`, "g");
  const m = re.exec(src);
  return m ? re.lastIndex : -1;
}

type ScanResult = { value: string; complete: boolean };

/** 从开引号之后 `start` 处扫描一个 JSON 字符串字面量，处理转义，支持中途截断。 */
function scanJsonString(src: string, start: number): ScanResult {
  let out = "";
  let i = start;
  while (i < src.length) {
    const ch = src[i]!;
    if (ch === "\\") {
      const next = src[i + 1];
      if (next === undefined) {
        // 结尾悬空的反斜杠：视为未完成
        return { value: out, complete: false };
      }
      switch (next) {
        case "n":
          out += "\n";
          break;
        case "t":
          out += "\t";
          break;
        case "r":
          out += "\r";
          break;
        case "b":
          out += "\b";
          break;
        case "f":
          out += "\f";
          break;
        case '"':
          out += '"';
          break;
        case "\\":
          out += "\\";
          break;
        case "/":
          out += "/";
          break;
        case "u": {
          const hex = src.slice(i + 2, i + 6);
          if (hex.length < 4) {
            return { value: out, complete: false };
          }
          out += String.fromCharCode(parseInt(hex, 16));
          i += 6;
          continue;
        }
        default:
          out += next;
          break;
      }
      i += 2;
      continue;
    }
    if (ch === '"') {
      return { value: out, complete: true };
    }
    out += ch;
    i += 1;
  }
  return { value: out, complete: false };
}

/** 提取一个「短」字符串字段（如 path / operation），仅在已完整闭合时返回。 */
function extractCompleteField(src: string, key: string): string | undefined {
  const start = findStringValueStart(src, key);
  if (start < 0) return undefined;
  const r = scanJsonString(src, start);
  return r.complete ? r.value : undefined;
}

export function parsePartialFileWrite(
  argsJson: string | null | undefined,
): PartialFileWrite | null {
  if (!argsJson) return null;
  const trimmed = argsJson.trim();
  if (!trimmed.startsWith("{")) return null;

  // 快路径：完整 JSON
  try {
    const obj = JSON.parse(trimmed) as Record<string, unknown>;
    return {
      path: typeof obj.path === "string" ? obj.path : undefined,
      operation: typeof obj.operation === "string" ? obj.operation : undefined,
      content: typeof obj.content === "string" ? obj.content : undefined,
      complete: true,
    };
  } catch {
    // 部分 JSON，转扫描
  }

  const path = extractCompleteField(trimmed, "path");
  const operation = extractCompleteField(trimmed, "operation");
  const contentStart = findStringValueStart(trimmed, "content");
  let content: string | undefined;
  let complete = false;
  if (contentStart >= 0) {
    const r = scanJsonString(trimmed, contentStart);
    content = r.value;
    complete = r.complete;
  }

  if (path === undefined && operation === undefined && content === undefined) {
    return null;
  }
  return { path, operation, content, complete };
}
