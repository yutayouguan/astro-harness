# 1 视频理解

> 来源：[Google AI for Developers](https://ai.google.dev/gemini-api/docs/video-understanding?hl=zh-cn)  
> 抓取说明：官方文档 **Interactions API** 版本（页面默认版本）。  
> 原文标题：视频理解

---

> 如需了解视频生成，请参阅 [Veo](https://ai.google.dev/gemini-api/docs/video?hl=zh-cn) 指南。

Gemini 模型可以处理视频，从而实现许多前沿开发者用例，而这些用例在过去需要使用特定于领域的模型。
Gemini 的一些视觉功能包括：描述视频、对视频进行分段和提取视频中的信息、回答有关视频内容的问题，以及引用视频中的特定时间戳。

您可以通过以下方式向 Gemini 提供视频作为输入：

| 输入法 | 最大大小 | 推荐的使用场景 |
| --- | --- | --- |
| [File API](#upload-video) | 20GB（付费）/ 2GB（免费） | 大型文件（100MB 以上）、长视频（10 分钟以上）、可重复使用的文件。 |
| [Cloud Storage 注册](https://ai.google.dev/gemini-api/docs/file-input-methods?hl=zh-cn#registration) | 2GB（每个文件，无存储空间限制） | 大型文件（100MB 以上）、长视频（10 分钟以上）、持久性文件、可重复使用的文件。 |
| [内嵌数据](#inline-video) | < 100MB | 小型文件（<100MB）、时长较短（<1 分钟）、一次性输入。 |
| [YouTube 网址](#youtube) | 不适用 | 公开的 YouTube 视频。 |

> **注意**：建议在大多数用例中使用 [File API](#upload-video)，尤其是对于大于 100MB 的文件，或者当您想在多个请求中重复使用文件时。

如需了解其他文件输入方法（例如使用外部网址或存储在 Google Cloud 中的文件），请参阅
[文件输入方法](https://ai.google.dev/gemini-api/docs/file-input-methods?hl=zh-cn)指南。

### 1.1.1 上传视频文件

以下代码会下载示例视频，使用 [Files API](https://ai.google.dev/gemini-api/docs/files?hl=zh-cn) 上传该视频，
等待其处理完毕，然后使用上传的文件引用来
总结视频。

### 1.1.2 Python

```python
from google import genai
import base64
import time

client = genai.Client()

myfile = client.files.upload(file="path/to/sample.mp4")

while not myfile.state or myfile.state.name != "ACTIVE":
    print("Processing video...")
    time.sleep(5)
    myfile = client.files.get(name=myfile.name)

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input=[
        {"type": "video", "uri": myfile.uri, "mime_type": myfile.mime_type},
        {"type": "text", "text": "Summarize this video. Then create a quiz with an answer key based on the information in this video."}
    ]
)

print(interaction.output_text)
```

### 1.1.3 JavaScript

```javascript
import { GoogleGenAI } from "@google/genai";

const ai = new GoogleGenAI({});

async function main() {
  const myfile = await ai.files.upload({
    file: "path/to/sample.mp4",
    config: { mimeType: "video/mp4" },
  });

  let getFile = await ai.files.get({ name: myfile.name });
  while (getFile.state === 'PROCESSING') {
      getFile = await ai.files.get({ name: myfile.name });
      console.log(`current file status: ${getFile.state}`);
      console.log('File is still processing, retrying in 5 seconds');

      await new Promise((resolve) => {
          setTimeout(resolve, 5000);
      });
  }
  if (getFile.state === 'FAILED') {
      throw new Error('File processing failed.');
  }

  const interaction = await ai.interactions.create({
    model: "gemini-3.5-flash",
    input: [
      { type: "video", uri: myfile.uri, mime_type: myfile.mimeType },
      { type: "text", text: "Summarize this video. Then create a quiz with an answer key based on the information in this video." }
    ],
  });
  console.log(interaction.output_text);
}

await main();
```

### 1.1.4 REST

```
VIDEO_PATH="path/to/sample.mp4"
MIME_TYPE=$(file -b --mime-type "${VIDEO_PATH}")
NUM_BYTES=$(wc -c < "${VIDEO_PATH}")
DISPLAY_NAME=VIDEO

tmp_header_file=upload-header.tmp

echo "Starting file upload..."
curl "https://generativelanguage.googleapis.com/upload/v1beta/files" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -D ${tmp_header_file} \
  -H "X-Goog-Upload-Protocol: resumable" \
  -H "X-Goog-Upload-Command: start" \
  -H "X-Goog-Upload-Header-Content-Length: ${NUM_BYTES}" \
  -H "X-Goog-Upload-Header-Content-Type: ${MIME_TYPE}" \
  -H "Content-Type: application/json" \
  -d "{'file': {'display_name': '${DISPLAY_NAME}'}}" 2> /dev/null

upload_url=$(grep -i "x-goog-upload-url: " "${tmp_header_file}" | cut -d" " -f2 | tr -d "\r")
rm "${tmp_header_file}"

echo "Uploading video data..."
curl "${upload_url}" \
  -H "Content-Length: ${NUM_BYTES}" \
  -H "X-Goog-Upload-Offset: 0" \
  -H "X-Goog-Upload-Command: upload, finalize" \
  --data-binary "@${VIDEO_PATH}" 2> /dev/null > file_info.json

file_uri=$(jq -r ".file.uri" file_info.json)
file_name=$(jq -r ".file.name" file_info.json)
echo file_uri=$file_uri

echo "File uploaded successfully. File URI: ${file_uri}"

# Polling loop
echo "Waiting for file to be processed..."
while true; do
  curl -s "https://generativelanguage.googleapis.com/v1beta/${file_name}" \
    -H "x-goog-api-key: $GEMINI_API_KEY" > file_status.json
  state=$(jq -r ".state" file_status.json)
  echo "Current state: $state"
  if [ "$state" == "ACTIVE" ]; then
    break
  elif [ "$state" == "FAILED" ]; then
    echo "File processing failed."
    exit 1
  fi
  sleep 5
done

echo "Generating content from video..."
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
    -H "x-goog-api-key: $GEMINI_API_KEY" \
    -H 'Content-Type: application/json' \
    -d '{
      "model": "gemini-3.5-flash",
      "input": [
        {"type": "video", "uri": "'${file_uri}'", "mime_type": "'${MIME_TYPE}'"},
        {"type": "text", "text": "Summarize this video. Then create a quiz with an answer key based on the information in this video."}
      ]
    }' 2> /dev/null > response.json

jq ".steps[].content[0].text" response.json
```

如果总请求大小（包括文件、文本提示、系统说明等）超过 20 MB、视频时长较长，或者您打算在多个提示中使用同一视频，请务必使用 Files API。
File API 直接接受视频文件格式。

如需详细了解如何使用媒体文件，请参阅
[Files API](https://ai.google.dev/gemini-api/docs/files?hl=zh-cn)。

### 1.1.5 以内嵌方式传递视频数据

您可以直接在请求中传递较小的视频，而无需使用 File API 上传视频文件。此方法适用于总请求大小不超过 20 MB 的较短视频。

以下是提供内嵌视频数据的示例：

### 1.1.6 Python

```python
from google import genai
import base64

video_file_name = "/path/to/your/video.mp4"
video_bytes = open(video_file_name, 'rb').read()

client = genai.Client()
interaction = client.interactions.create(
    model='gemini-3.5-flash',
    input=[
        {"type": "text", "text": "Please summarize the video in 3 sentences."},
        {
            "type": "video",
            "data": base64.b64encode(video_bytes).decode('utf-8'),
            "mime_type": "video/mp4"
        }
    ]
)
print(interaction.output_text)
```

### 1.1.7 JavaScript

```javascript
import { GoogleGenAI } from "@google/genai";
import * as fs from "node:fs";

const ai = new GoogleGenAI({});
const base64VideoFile = fs.readFileSync("path/to/small-sample.mp4", {
  encoding: "base64",
});

const interaction = await ai.interactions.create({
  model: "gemini-3.5-flash",
  input: [
    { type: "text", text: "Please summarize the video in 3 sentences." },
    {
      type: "video",
      data: base64VideoFile,
      mime_type: "video/mp4",
    }
  ],
});
console.log(interaction.output_text);
```

### 1.1.8 REST

> [!NOTE]
> `Argument list too long`

```
VIDEO_PATH=/path/to/your/video.mp4

if [[ "$(base64 --version 2>&1)" = *"FreeBSD"* ]]; then
  B64FLAGS="--input"
else
  B64FLAGS="-w0"
fi

curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
    -H "x-goog-api-key: $GEMINI_API_KEY" \
    -H 'Content-Type: application/json' \
    -d '{
      "model": "gemini-3.5-flash",
      "input": [
        {"type": "text", "text": "Please summarize the video in 3 sentences."},
        {
          "type": "video",
          "data": "'$(base64 $B64FLAGS $VIDEO_PATH)'",
          "mime_type": "video/mp4"
        }
      ]
    }' 2> /dev/null
```

### 1.1.9 传递 YouTube 网址

您可以将 YouTube 网址直接传递给 Gemini API，作为请求的一部分，如下所示：

### 1.1.10 Python

```python
from google import genai

client = genai.Client()
interaction = client.interactions.create(
    model='gemini-3.5-flash',
    input=[
        {"type": "text", "text": "Please summarize the video in 3 sentences."},
        {
            "type": "video",
            "uri": "https://www.youtube.com/watch?v=9hE5-98ZeCg"
        }
    ]
)
print(interaction.output_text)
```

### 1.1.11 JavaScript

```javascript
import { GoogleGenAI } from "@google/genai";

const ai = new GoogleGenAI({});

const interaction = await ai.interactions.create({
  model: "gemini-3.5-flash",
  input: [
    { type: "text", text: "Please summarize the video in 3 sentences." },
    {
      type: "video",
      uri: "https://www.youtube.com/watch?v=9hE5-98ZeCg",
    }
  ],
});
console.log(interaction.output_text);
```

### 1.1.12 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
    -H "x-goog-api-key: $GEMINI_API_KEY" \
    -H 'Content-Type: application/json' \
    -d '{
      "model": "gemini-3.5-flash",
      "input": [
        {"type": "text", "text": "Please summarize the video in 3 sentences."},
        {
          "type": "video",
          "uri": "https://www.youtube.com/watch?v=9hE5-98ZeCg"
        }
      ]
    }' 2> /dev/null
```

**限制** ：

- 对于免费层级，您每天上传的 YouTube 视频时长不得超过 8 小时。
- 对于付费层级，没有基于视频时长的限制。
- 对于 Gemini 2.5 之前的模型，您每次请求只能上传 1 个视频。对于 Gemini 2.5 及更高版本的模型，您每次请求最多能上传 10 个视频。
- 您只能上传公开视频（不能上传私享视频或未公开列出的视频）。

## 1.2 引用内容中的时间戳

您可以使用 `MM:SS` 格式的时间戳，针对视频中的特定时间点提出问题。

### 1.2.1 Python

```
prompt = "What are the examples given at 00:05 and 00:10 supposed to show us?"
```

### 1.2.2 JavaScript

```
const prompt = "What are the examples given at 00:05 and 00:10 supposed to show us?";
```

### 1.2.3 REST

```
PROMPT="What are the examples given at 00:05 and 00:10 supposed to show us?"
```

## 1.3 从视频中提取详细分析洞见

Gemini 模型通过处理**音频和视觉** 信息流中的信息，提供强大的视频内容理解功能。这让您可以提取丰富多样的详细信息，包括生成视频中发生情况的描述，以及回答有关视频内容的问题。

对于视觉描述，模型以 **1 帧/秒** (FPS) 的速率对视频进行采样。此默认采样率适用于大多数内容，但请注意，它可能会遗漏快速移动或场景快速变化的视频中的细节。

### 1.3.1 Python

```
prompt = "Describe the key events in this video, providing both audio and visual details. Include timestamps for salient moments."
```

### 1.3.2 JavaScript

```
const prompt = "Describe the key events in this video, providing both audio and visual details. Include timestamps for salient moments.";
```

### 1.3.3 REST

```
PROMPT="Describe the key events in this video, providing both audio and visual details. Include timestamps for salient moments."
```

## 1.4 支持的视频格式

Gemini 支持以下视频格式 MIME 类型：

- `video/mp4`
- `video/mpeg`
- `video/mov`
- `video/avi`
- `video/x-flv`
- `video/mpg`
- `video/webm`
- `video/wmv`
- `video/3gpp`

## 1.5 有关视频技术方面的详细信息

- **支持的模型和上下文** ：所有 Gemini 模型都可以处理视频数据。
 - 上下文窗口为 100 万个 token 的模型可以处理时长不超过 1 小时（默认媒体分辨率）或 3 小时（低媒体分辨率）的视频。
- **File API 处理**：使用 File API 时，视频的存储速率为 1
帧/秒 (FPS)，音频的处理速率则为 1Kbps（单声道）。
每秒都会添加时间戳。
 - 为了改进推理，这些速率将来可能会发生变化。
- **token 计算**：视频的每一秒都按如下方式计算 token：
 - 各帧（选段率为 1 FPS）：
 - 如果 `media_resolution` 设置为低，则每帧按 66 个 token 计算。
 - 否则，每帧按 258 个 token 计算。
 - 音频：每秒按 32 个 token 计算。
 - 元数据也包含在内。
 - 总计：默认媒体分辨率下，每秒视频大约需要 300 个 token；低媒体分辨率下，每秒视频大约需要 100 个 token。
- **媒体分辨率**：Gemini 3 引入了使用 `media_resolution` 参数对多模态
视觉处理进行精细控制的功能。`media_resolution` 参数用于确定**为每个输入图片或视频帧分配的 token 数量上限** 。分辨率越高，模型读取精细文本或识别细小细节的能力就越强，但 token 用量和延迟也会增加。如需详细了解 token 计算，请参阅 [token](https://ai.google.dev/gemini-api/docs/tokens?hl=zh-cn) 指南。
- **时间戳格式**：在提示中引用视频中的特定时刻时，请使用 `MM:SS` 格式（例如，`01:15` 表示 1 分 15 秒）。
- **最佳实践**：
 - 为获得最佳效果，每个提示请求仅使用一个视频。
 - 如果将文本与单个视频相结合，请在 `input` 数组中将文本提示放在视频部分之后 。
 - 请注意，如果选段率为 1 FPS，快速动作序列可能会丢失细节。如有必要，可以考虑放慢此类片段的播放速度。

## 1.6 后续步骤

本指南介绍了如何上传视频文件以及如何从视频输入生成文本输出。如需了解详情，请参阅以下资源：

- [系统说明](https://ai.google.dev/gemini-api/docs/text-generation?hl=zh-cn#system-instructions)：
系统说明可让您根据其
特定需求和使用情形来控制模型的行为。
- [Files API](https://ai.google.dev/gemini-api/docs/files?hl=zh-cn)：详细了解如何上传和管理
文件以供 Gemini 使用。
- [文件提示策略](https://ai.google.dev/gemini-api/docs/files?hl=zh-cn#prompt-guide)：
Gemini API 支持使用文本、图片、音频和视频数据进行提示，也
称为多模态提示。
- [安全指南](https://ai.google.dev/gemini-api/docs/safety-guidance?hl=zh-cn)：有时，生成式
 AI 模型会生成意外输出，例如不准确、
有偏见或令人反感的输出。后处理和人工评估对于
限制此类输出造成的危害风险至关重要。
