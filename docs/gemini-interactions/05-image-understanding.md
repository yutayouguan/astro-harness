# 1 图片理解 / 图片推理

> 来源：[Google AI for Developers](https://ai.google.dev/gemini-api/docs/image-understanding?hl=zh-cn)  
> 抓取说明：官方文档 **Interactions API** 版本（页面默认版本）。  
> 原文标题：图片理解

---

Gemini 模型从一开始就具有多模态特性，可用于执行各种图像处理和计算机视觉任务，包括但不限于图片说明、分类和视觉问答，而无需训练专门的机器学习模型。

除了具备一般多模态功能之外，Gemini 模型还通过额外训练，针对特定用例（例如[对象检测](#object-detection)和[细分](#segmentation)）提供**更高的准确度**。

## 1.1 将图片传递给 Gemini

您可以使用多种方法将图片作为输入内容提供给 Gemini：

- [使用网址传递图片](#url-image)：非常适合可公开访问的图片。
- [传递内嵌图片数据](#inline-image)：用于传递 base64 编码的图片数据。
- [使用 File API 上传图片](#upload-image)：建议用于较大的文件，或在多个请求中重复使用图片。

### 1.1.1 使用网址传递图片

您可以使用 [Files API](https://ai.google.dev/gemini-api/docs/files?hl=zh-cn) 上传图片，并在请求中传递该图片：

### 1.1.2 Python

```python
from google import genai

client = genai.Client()

uploaded_file = client.files.upload(file="path/to/organ.jpg")

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input=[
        {"type": "text", "text": "Caption this image."},
        {
            "type": "image",
            "uri": uploaded_file.uri,
            "mime_type": uploaded_file.mime_type
        }
    ]
)
print(interaction.output_text)
```

### 1.1.3 JavaScript

```javascript
import { GoogleGenAI } from "@google/genai";

const client = new GoogleGenAI({});

const uploadedFile = await client.files.upload({
    file: "path/to/organ.jpg",
    config: { mimeType: "image/jpeg" }
});

const interaction = await client.interactions.create({
    model: "gemini-3.5-flash",
    input: [
        {type: "text", text: "Caption this image."},
        {
            type: "image",
            uri: uploadedFile.uri,
            mime_type: uploadedFile.mimeType
        }
    ]
});
console.log(interaction.output_text);
```

### 1.1.4 REST

```
# First upload the file using the Files API, then use the URI:
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gemini-3.5-flash",
    "input": [
      {"type": "text", "text": "Caption this image."},
      {
        "type": "image",
        "uri": "YOUR_FILE_URI",
        "mime_type": "image/jpeg"
      }
    ]
  }'
```

### 1.1.5 传递内嵌图片数据

您可以以 base64 编码的字符串形式提供图片数据：

### 1.1.6 Python

```python
import base64
from google import genai

with open('path/to/small-sample.jpg', 'rb') as f:
    image_bytes = f.read()

client = genai.Client()

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input=[
        {"type": "text", "text": "Caption this image."},
        {
            "type": "image",
            "data": base64.b64encode(image_bytes).decode('utf-8'),
            "mime_type": "image/jpeg"
        }
    ]
)
print(interaction.output_text)
```

### 1.1.7 JavaScript

```javascript
import { GoogleGenAI } from "@google/genai";
import * as fs from "node:fs";

const client = new GoogleGenAI({});
const base64ImageFile = fs.readFileSync("path/to/small-sample.jpg", {
  encoding: "base64",
});

const interaction = await client.interactions.create({
    model: "gemini-3.5-flash",
    input: [
        {type: "text", text: "Caption this image."},
        {
            type: "image",
            data: base64ImageFile,
            mime_type: "image/jpeg"
        }
    ]
});
console.log(interaction.output_text);
```

### 1.1.8 REST

```
IMG_PATH="/path/to/your/image1.jpg"

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
      {"type": "text", "text": "Caption this image."},
      {
        "type": "image",
        "data": "'"$(base64 $B64FLAGS $IMG_PATH)"'",
        "mime_type": "image/jpeg"
      }
    ]
  }'
```

### 1.1.9 使用 File API 上传图片

对于大型文件，或者为了能够重复使用同一图片文件，请使用 Files API。请参阅 [Files API 指南](https://ai.google.dev/gemini-api/docs/files?hl=zh-cn)。

### 1.1.10 Python

```python
from google import genai

client = genai.Client()

my_file = client.files.upload(file="path/to/sample.jpg")

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input=[
        {"type": "text", "text": "Caption this image."},
        {
            "type": "image",
            "uri": my_file.uri,
            "mime_type": my_file.mime_type
        }
    ]
)
print(interaction.output_text)
```

### 1.1.11 JavaScript

```javascript
import { GoogleGenAI } from "@google/genai";

const client = new GoogleGenAI({});

const myfile = await client.files.upload({
    file: "path/to/sample.jpg",
    config: { mimeType: "image/jpeg" },
});

const interaction = await client.interactions.create({
    model: "gemini-3.5-flash",
    input: [
        {type: "text", text: "Caption this image."},
        {
            type: "image",
            uri: myfile.uri,
            mime_type: myfile.mimeType
        }
    ]
});
console.log(interaction.output_text);
```

### 1.1.12 REST

```
# First upload the file (see Files API guide for details)
# Then use the file URI in the request:

curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gemini-3.5-flash",
    "input": [
      {"type": "text", "text": "Caption this image."},
      {
        "type": "image",
        "uri": "YOUR_FILE_URI",
        "mime_type": "image/jpeg"
      }
    ]
  }'
```

## 1.2 使用多张图片进行提示

您可以在单个提示中提供多张图片，只需在 `input` 数组中添加多个图片对象即可：

### 1.2.1 Python

```python
from google import genai

client = genai.Client()

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input=[
        {"type": "text", "text": "What is different between these two images?"},
        {
            "type": "image",
            "uri": "https://example.com/image1.jpg",
            "mime_type": "image/jpeg"
        },
        {
            "type": "image",
            "uri": "https://example.com/image2.jpg",
            "mime_type": "image/jpeg"
        }
    ]
)
print(interaction.output_text)
```

### 1.2.2 JavaScript

```javascript
import { GoogleGenAI } from "@google/genai";

const client = new GoogleGenAI({});

const interaction = await client.interactions.create({
    model: "gemini-3.5-flash",
    input: [
        {type: "text", text: "What is different between these two images?"},
        {
            type: "image",
            uri: "https://example.com/image1.jpg",
            mime_type: "image/jpeg"
        },
        {
            type: "image",
            uri: "https://example.com/image2.jpg",
            mime_type: "image/jpeg"
        }
    ]
});
console.log(interaction.output_text);
```

### 1.2.3 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gemini-3.5-flash",
    "input": [
      {"type": "text", "text": "What is different between these two images?"},
      {
        "type": "image",
        "uri": "https://example.com/image1.jpg",
        "mime_type": "image/jpeg"
      },
      {
        "type": "image",
        "uri": "https://example.com/image2.jpg",
        "mime_type": "image/jpeg"
      }
    ]
  }'
```

## 1.3 对象检测

模型经过训练，可以检测图片中的对象并获取其边界框坐标。相对于图片尺寸的坐标，缩放至 [0, 1000]。您需要根据原始图片大小对这些坐标进行反缩放。

> [!NOTE]
> `zod`
> `npm install zod`

### 1.3.1 Python

```python
from google import genai
from pydantic import BaseModel, Field
from typing import List
import json

client = genai.Client()
prompt = "Detect the all of the prominent items in the image. The box_2d should be [ymin, xmin, ymax, xmax] normalized to 0-1000."

class BoundingBox(BaseModel):
    box_2d: List[int] = Field(description="The 2D bounding box of the item as [ymin, xmin, ymax, xmax] normalized to 0-1000.")
    mask: List[List[int]] = Field(description="The segmentation mask of the item as a polygon of [x,y] coordinates, normalized to 0-1000.")
    label: str = Field(description="A descriptive label for the item.")

class BoundingBoxes(BaseModel):
    boxes: List[BoundingBox]

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input=[
        {"type": "text", "text": prompt},
        {
            "type": "image",
            "uri": "https://example.com/image.png",
            "mime_type": "image/png"
        }
    ],
    response_format={
        "type": "text",
        "mime_type": "application/json",
        "schema": BoundingBoxes.model_json_schema()
    }
)

bounding_boxes = BoundingBoxes.model_validate_json(interaction.output_text)
print(bounding_boxes)
```

### 1.3.2 JavaScript

```javascript
import { GoogleGenAI } from "@google/genai";
import * as z from "zod";

const client = new GoogleGenAI({});
const prompt = "Detect the all of the prominent items in the image. The box_2d should be [ymin, xmin, ymax, xmax] normalized to 0-1000.";

const boundingBoxesSchema = z.object({
  boxes: z.array(z.object({
    box_2d: z.array(z.number()),
    mask: z.array(z.array(z.number())),
    label: z.string()
  }))
});

const interaction = await client.interactions.create({
  model: "gemini-3.5-flash",
  input: [
    { type: "text", text: prompt },
    {
      type: "image",
      uri: "https://example.com/image.png",
      mime_type: "image/png"
    }
  ],
  response_format: {
    type: 'text',
    mime_type: 'application/json',
    schema: z.toJSONSchema(boundingBoxesSchema)
  },
});

const result = boundingBoxesSchema.parse(JSON.parse(interaction.output_text));
console.log(result);
```

### 1.3.3 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gemini-3.5-flash",
    "input": [
      {"type": "text", "text": "Detect the all of the prominent items in the image. The box_2d should be [ymin, xmin, ymax, xmax] normalized to 0-1000."},
      {
        "type": "image",
        "uri": "https://example.com/image.png",
        "mime_type": "image/png"
      }
    ],
    "response_format": {
      "type": "text",
      "mime_type": "application/json",
      "schema": {
        "type": "object",
        "properties": {
          "boxes": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "box_2d": { "type": "array", "items": { "type": "integer" } },
                "mask": { "type": "array", "items": { "type": "array", "items": { "type": "integer" } } },
                "label": { "type": "string" }
              },
              "required": ["box_2d", "mask", "label"]
            }
          }
        },
        "required": ["boxes"]
      }
    }
  }'
```

如需查看更多示例，请访问 [Gemini Cookbook](https://github.com/google-gemini/cookbook)。

## 1.4 细分

Gemini 模型不仅可以检测商品，还可以对商品进行分割并提供其轮廓遮罩。

模型会预测一个 JSON 列表，其中每个项都表示一个分割掩码。每个商品都有一个边界框 (“`box_2d`”)，其格式为 `[ymin, xmin, ymax, xmax]`，包含介于 0 到 1000 之间的归一化坐标；一个用于标识对象的标签 (“`label`”)；最后是边界框内的分割掩码，以归一化为 0-1000 的 `[x, y]` 坐标多边形表示。

### 1.4.1 Python

```python
from google import genai
from pydantic import BaseModel, Field
from typing import List
import json

client = genai.Client()

prompt = """
Give the segmentation masks for the wooden and glass items.
Output a JSON list of segmentation masks where each entry contains the 2D
bounding box in the key "box_2d", the segmentation mask in key "mask", and
the text label in the key "label". Use descriptive labels.
"""

class BoundingBox(BaseModel):
    box_2d: List[int] = Field(description="The 2D bounding box of the item as [ymin, xmin, ymax, xmax] normalized to 0-1000.")
    mask: List[List[int]] = Field(description="The segmentation mask of the item as a polygon of [x,y] coordinates, normalized to 0-1000.")
    label: str = Field(description="A descriptive label for the item.")

class BoundingBoxes(BaseModel):
    boxes: List[BoundingBox]

interaction = client.interactions.create(
    model="gemini-3.5-flash",
    input=[
        {"type": "text", "text": prompt},
        {
            "type": "image",
            "uri": "https://example.com/image.png",
            "mime_type": "image/png"
        }
    ],
    response_format={
        "type": "text",
        "mime_type": "application/json",
        "schema": BoundingBoxes.model_json_schema()
    },
    generation_config={
        "thinking_level": "minimal"
    }
)

items = BoundingBoxes.model_validate_json(interaction.output_text)
print("Segmentation results:", items)
```

### 1.4.2 JavaScript

```javascript
import { GoogleGenAI } from "@google/genai";
import * as z from "zod";

const client = new GoogleGenAI({});
const prompt = `
Give the segmentation masks for the wooden and glass items.
Output a JSON list of segmentation masks where each entry contains the 2D
bounding box in the key "box_2d", the segmentation mask in key "mask", and
the text label in the key "label". Use descriptive labels.
`;

const boundingBoxesSchema = z.object({
  boxes: z.array(z.object({
    box_2d: z.array(z.number()),
    mask: z.array(z.array(z.number())),
    label: z.string()
  }))
});

const interaction = await client.interactions.create({
  model: "gemini-3.5-flash",
  input: [
    { type: "text", text: prompt },
    {
      type: "image",
      uri: "https://example.com/image.png",
      mime_type: "image/png"
    }
  ],
  response_format: {
    type: 'text',
    mime_type: 'application/json',
    schema: z.toJSONSchema(boundingBoxesSchema)
  },
  generation_config: {
    thinking_level: "minimal"
  }
});

const result = boundingBoxesSchema.parse(JSON.parse(interaction.output_text));
console.log(result);
```

### 1.4.3 REST

```bash
curl -X POST "https://generativelanguage.googleapis.com/v1beta/interactions" \
  -H "x-goog-api-key: $GEMINI_API_KEY" \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "gemini-3.5-flash",
    "input": [
      {"type": "text", "text": "Give the segmentation masks for the wooden and glass items.\nOutput a JSON list of segmentation masks where each entry contains the 2D\nbounding box in the key \"box_2d\", the segmentation mask in key \"mask\", and\nthe text label in the key \"label\". Use descriptive labels."},
      {
        "type": "image",
        "uri": "https://example.com/image.png",
        "mime_type": "image/png"
      }
    ],
    "response_format": {
      "type": "text",
      "mime_type": "application/json",
      "schema": {
        "type": "object",
        "properties": {
          "boxes": {
            "type": "array",
            "items": {
              "type": "object",
              "properties": {
                "box_2d": { "type": "array", "items": { "type": "integer" } },
                "mask": { "type": "array", "items": { "type": "array", "items": { "type": "integer" } } },
                "label": { "type": "string" }
              },
              "required": ["box_2d", "mask", "label"]
            }
          }
        },
        "required": ["boxes"]
      }
    },
    "generation_config": {
      "thinking_level": "minimal"
    }
  }'
```

![一张放有纸杯蛋糕的桌子，其中木质和玻璃物体突出显示](https://ai.google.dev/static/gemini-api/docs/images/segmentation.jpg?hl=zh-cn)

## 1.5 支持的图片格式

Gemini 支持以下图片格式 MIME 类型：

- PNG - `image/png`
- JPEG - `image/jpeg`
- WEBP - `image/webp`
- HEIC - `image/heic`
- HEIF - `image/heif`

如需了解其他文件输入方法，请参阅[文件输入方法](https://ai.google.dev/gemini-api/docs/file-input-methods?hl=zh-cn)指南。

## 1.6 功能

所有 Gemini 模型版本都是多模态模型，可用于各种图像处理和计算机视觉任务，包括但不限于图片说明、视觉问答、图片分类、对象检测和分割。

Gemini 可以减少对专用机器学习模型的需求，具体取决于您的质量和性能要求。

除了通用功能外，最新模型版本还经过专门训练，可提高专业任务的准确性，例如增强的[对象检测](#object-detection)和[分割](#segmentation)。

## 1.7 限制和关键技术信息

### 1.7.1 文件限制

Gemini 模型支持每个请求最多上传 3,600 个图片文件。

### 1.7.2 token 计算

- 如果两个维度均小于或等于 384 像素，则为 258 个 token。
较大的图片会被分块为 768x768 像素的图块，每个图块需要 258 个 token。

计算图块数量的粗略公式如下：

- 计算裁剪单元大小（约为：`floor(min(width, height)` / 1.5）。
- 将每个维度除以裁剪单元大小，然后将结果相乘，即可得到图块数量。

例如，对于尺寸为 960x540 的图片，剪裁单元尺寸为 360。将每个维度除以 360，得到的图块数量为 3 * 2 = 6。

### 1.7.3 媒体分辨率

Gemini 3 通过 `media_resolution` 参数引入了对多模态视觉处理的精细控制。`media_resolution` 参数用于确定**为每个输入图片或视频帧分配的 token 数量上限**。分辨率越高，模型读取细小文字或识别细微细节的能力就越强，但 token 用量和延迟时间也会增加。

## 1.8 技巧和最佳做法

- 验证图片是否已正确旋转。
- 使用清晰且不模糊的图片。
- 如果使用包含文本的单张图片，请在 `input` 数组中将文本提示放在图片之前。

## 1.9 后续步骤

本指南将介绍如何上传图片文件并根据图片输入生成文本输出。如需了解详情，请参阅以下资源：

- [Files API](https://ai.google.dev/gemini-api/docs/files?hl=zh-cn)：详细了解如何上传和管理文件以供 Gemini 使用。
- [系统指令](https://ai.google.dev/gemini-api/docs/text-generation?hl=zh-cn#system-instructions)：系统指令可让您根据自己的特定需求和使用情形来控制模型的行为。
- [文件提示策略](https://ai.google.dev/gemini-api/docs/files?hl=zh-cn#prompt-guide)：Gemini API 支持使用文本、图片、音频和视频数据进行提示，也称为多模态提示。
- [安全指南](https://ai.google.dev/gemini-api/docs/safety-guidance?hl=zh-cn)：生成式 AI 模型有时会生成意料之外的输出，例如不准确、有偏见或令人反感的输出。后处理和人工评估对于限制此类输出造成的危害风险至关重要。
