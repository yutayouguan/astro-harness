# Azure OpenAI 实时音频：WebRTC

> 来源：https://learn.microsoft.com/zh-cn/azure/foundry/openai/how-to/realtime-audio-webrtc
> 拉取日期：2026-09-01
> 版权与更新以 Microsoft Learn 原页面为准。

Azure用于语音和音频的 OpenAI GPT 实时 API 是 GPT-4o 模型系列的一部分，支持低延迟的“语音传入，语音传出”对话交互。

可以通过 WebRTC、SIP 或 WebSocket 使用实时 API 将音频输入发送到模型并实时接收音频响应。 按照本文中的说明通过 WebRTC 开始使用实时 API。

在大多数情况下，使用 WebRTC API 进行实时音频流式处理。 WebRTC API 是一种 Web 标准，可在浏览器和移动应用程序之间实现实时通信（RTC）。 下面是 WebRTC 首选实时音频流式传输的一些原因：

* **较低的延迟**：WebRTC 旨在最大程度地减少延迟，使其更适用于音频和视频通信，其中低延迟对于保持质量和同步至关重要。
* **媒体处理**：WebRTC 内置了对音频和视频编解码器的支持，提供对媒体流的优化处理。
* **错误更正**：WebRTC 包括处理数据包丢失和抖动的机制，这对于通过不可预知的网络保持音频流的质量至关重要。
* **对等通信**：WebRTC 允许客户端之间直接通信，减少中央服务器中继音频数据的需求，从而进一步降低延迟。

在以下情况下通过 WebSocket 使用实时 API：

* 将音频数据从服务器流式传输到客户端。
* 在客户端和服务器之间实时发送和接收数据。

不建议使用 WebSocket 进行实时音频流式处理，因为它们的延迟高于 WebRTC。

## 支持的模型

可以访问 GPT 实时模型，以便在 [美国东部 2 和瑞典中部区域](../../foundry-models/concepts/models-sold-directly-by-azure-region-availability)进行全球部署。

* `gpt-4o-mini-realtime-preview` (2024-12-17)
* `gpt-4o-realtime-preview` (2024-12-17)
* `gpt-realtime` （版本 2025-08-28）
* `gpt-realtime-mini` （版本 2025-10-06）
* `gpt-realtime-mini` （版本 2025-12-15）
* `gpt-realtime-1.5` （版本 2026-02-23）
* `gpt-realtime-2` （版本 2026-05-07）
* `gpt-realtime-translate` （版本 2026-05-06）
* `gpt-realtime-whisper` （版本 2026-05-06）
* `gpt-live-transcribe` （版本 2026-07-29）

`/openai/v1`调用实时 API 时，请使用请求 URL 中的路径。

有关支持的模型的详细信息，请参阅 [模型和版本文档](about:blank/foundry-models/concepts/models-sold-directly-by-azure#audio-models)。

重要

使用 WebRTC 的 GA 协议。

本文中所述的 GA 终结点使用与预览 API 不同的 URL：

* GA 使用 `/openai/v1/realtime/client_secrets` 和 `/openai/v1/realtime/calls`
* 已弃用的预览版使用了 `/openai/realtimeapi/sessions` 和区域 URL

如果你是当前使用预览终结点的客户，请[迁移到正式版协议](realtime-audio-preview-api-migration-guide)。

我们 [在此处](/zh-cn/previous-versions/azure/foundry-models/realtime-audio-webrtc-legacy)保留旧协议文档。

## 先决条件

在使用 GPT 实时音频之前，需要：

* Azure订阅 - [免费创建一个订阅](https://azure.microsoft.com/pricing/purchase-options/azure-account?cid=msft_learn)。
* Microsoft Foundry 资源 - [在支持的区域之一中创建 Microsoft Foundry 资源](/zh-cn/azure/ai-services/multi-service-resource?pivots=azportal)。
* 如本文支持的模型部分所述，在受支持的区域中部署 GPT 实时 [模型](#supported-models) 。
  + 在 Foundry 门户中，加载项目。 在右上方菜单中选择“ **生成** ”，然后选择左窗格中的“ **模型** ”选项卡，然后选择“ **部署基本模型**”。 搜索所需的模型，然后选择“模型”页上的“ **部署** ”。

## 设置 WebRTC

若要使用 WebRTC，需要两段代码：

1. Web 浏览器应用程序。
2. 一种可通过 Web 浏览器检索临时令牌的服务。

其他选项：

* 通过检索临时令牌的同一服务，经由会话描述协议代理 Web 浏览器的会话协商。 此方案更安全，因为 Web 浏览器无权访问临时令牌。
* 使用查询参数筛选转到 Web 浏览器的消息。
* 创建观察者 WebSocket 连接以监听或记录会话。

## 步骤

### 步骤 1：设置服务以获取临时令牌

生成临时令牌的关键是使用 REST API

```
url = https://{your azure resource}.openai.azure.com/openai/v1/realtime/client_secrets
```

注意

这是 WebRTC 的 **GA 终结点** 。 如果要从预览版 API 迁移，终结点已更改：

* **预览版（已弃用）**： `/openai/realtimeapi/sessions?api-version=2025-04-01-preview`
* **GA （当前）**： `/openai/v1/realtime/client_secrets` （不需要 API 版本参数）

有关迁移详细信息，请参阅 [从预览版迁移到正式版实时 API](about:blank/realtime-audio-preview-api-migration-guide#update-webrtc-endpoints)。

你可以将此 URL 与你的 API 密钥或 Microsoft Entra ID 令牌一起使用。 此请求检索一个短暂使用的临时令牌，并设置您希望 Web 浏览器采用的会话配置，包括提示说明和输出语音。

下面是令牌服务的一些示例Python代码。 Web 浏览器应用程序可以使用 /token 终结点来调用此服务，以检索临时令牌。 此示例代码使用 DefaultAzureCredential 对生成临时令牌的 RealtimeAPI 进行身份验证。

注意

替换代码示例中的占位符值：

* `<your azure resource>` 或 `<YOUR AZURE RESOURCE>` - Azure OpenAI 资源名称
* `<your model deployment name>` 或 `<YOUR MODEL DEPLOYMENT NAME>` - 实时模型部署名称

会话配置包括：

| 领域 | 必填 | 描述 |
| --- | --- | --- |
| `session.type` | 是的 | 必须是 `realtime` |
| `session.model` | 是的 | 您的模型部署名称 |
| `session.instructions` | 不 | 助理的系统提示 |
| `session.audio.output.voice` | 不 | 音频输出的语音：`alloy`、`ash`、`ballad`、`coral`、`echo`、`sage`、`shimmer` 或 `verse` |

```
from flask import Flask, jsonify

import os
import requests
import time
import threading

from azure.identity import DefaultAzureCredential

app = Flask(__name__)

# Session configuration
session_config = {
    "session": {
        "type": "realtime",
        "model": "<your model deployment name>",
        "instructions": "You are a helpful assistant.",
        "audio": {
            "output": {
                "voice": "marin",
            },
        },
    },
}

# Get configuration from environment variables
azure_resource = os.getenv('AZURE_RESOURCE')  # e.g., 'your-azure-resource'

# Token caching variables
cached_token = None
token_expiry = 0
token_lock = threading.Lock()

def get_bearer_token(resource_scope: str) -> str:
    """Get a bearer token using DefaultAzureCredential with caching."""
    global cached_token, token_expiry

    current_time = time.time()

    # Check if we have a valid cached token (with 5 minute buffer before expiry)
    with token_lock:
        if cached_token and current_time < (token_expiry - 300):
            return cached_token

    # Get a new token
    try:
        credential = DefaultAzureCredential()
        token = credential.get_token(resource_scope)

        with token_lock:
            cached_token = token.token
            token_expiry = token.expires_on

        print(f"Acquired new bearer token, expires at: {time.ctime(token_expiry)}")
        return cached_token

    except Exception as e:
        print(f"Failed to acquire bearer token: {e}")
        raise

@app.route('/token', methods=['GET'])
def get_token():
    """
    An endpoint which returns the contents of a REST API request to the protected endpoint.
    Uses DefaultAzureCredential for authentication with token caching.
    """
    try:
        # Get bearer token using DefaultAzureCredential
        bearer_token = get_bearer_token("https://ai.azure.com/.default")

        # Construct the Azure OpenAI endpoint URL
        url = f"https://{azure_resource}.openai.azure.com/openai/v1/realtime/client_secrets"

        headers = {
            "Authorization": f"Bearer {bearer_token}",
            "Content-Type": "application/json",
        }

        # Make the request to Azure OpenAI
        response = requests.post(
            url,
            headers=headers,
            json=session_config,
            timeout=30
        )

        # Check if the request was successful
        if response.status_code != 200:
            print(f"Request failed with status {response.status_code}: {response.reason}")
            print(f"Response headers: {dict(response.headers)}")
            print(f"Response content: {response.text}")

        response.raise_for_status()

        # Parse the JSON response and extract the ephemeral token
        data = response.json()
        ephemeral_token = data.get('value', '')

        if not ephemeral_token:
            print(f"No ephemeral token found in response: {data}")
            return jsonify({"error": "No ephemeral token available"}), 500

        # Return the ephemeral token as JSON
        return jsonify({"token": ephemeral_token})

    except requests.exceptions.RequestException as e:
        print(f"Token generation error: {e}")
        if hasattr(e, 'response') and e.response is not None:
            print(f"Response status: {e.response.status_code}")
            print(f"Response reason: {e.response.reason}")
            print(f"Response content: {e.response.text}")
        return jsonify({"error": "Failed to generate token"}), 500
    except Exception as e:
        print(f"Unexpected error: {e}")
        return jsonify({"error": "Failed to generate token"}), 500

if __name__ == '__main__':
    if not azure_resource:
        print("Error: AZURE_RESOURCE environment variable is required")
        exit(1)

    print(f"Starting token service for Azure resource: {azure_resource}")
    print("Using DefaultAzureCredential for authentication")
    print("Production mode - use gunicorn to run this service:")

    port = int(os.getenv('PORT', 5000))
    print(f"  gunicorn -w 4 -b 0.0.0.0:{port} --timeout 30 token-service:app")
```

### 步骤 2：设置浏览器应用程序

浏览器应用程序调用令牌服务以获取令牌，然后启动与 RealtimeAPI 的 WebRTC 连接。 若要启动 WebRTC 连接，请使用以下 URL 和临时令牌进行身份验证。

```
https://<your azure resource>.openai.azure.com/openai/v1/realtime/calls
```

注意

这是 WebRTC 连接的 **GA 终结点** 。 如果要从预览版 API 迁移：

* **预览版（已弃用）**： `https://<region>.realtimeapi-preview.ai.azure.com/v1/realtimertc`
* **正式版（当前）**： `https://<your azure resource>.openai.azure.com/openai/v1/realtime/calls`

连接后，浏览器应用程序通过数据通道发送文本，并通过媒体通道发送音频。 下面是一个示例 HTML 文档，可帮助你入门。

```
<!DOCTYPE html>
    <html lang="en">
    <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>Azure OpenAI Realtime Session</title>
    </head>
    <body>
        <h1>Azure OpenAI Realtime Session</h1>
        <button onclick="StartSession()">Start Session</button>

        <!-- Log container for API messages -->
        <div id="logContainer"></div>

        <script>

        const AZURE_RESOURCE = "<your azure resource>"
        const WEBRTC_URL= `https://${AZURE_RESOURCE}.openai.azure.com/openai/v1/realtime/calls?webrtcfilter=on`

        async function StartSession() {
            try {

                // Call our token service to get the ephemeral key
                const tokenResponse = await fetch("/token");

                if (!tokenResponse.ok) {
                    throw new Error(`Token service request failed: ${tokenResponse.status}`);
                }

                const tokenData = await tokenResponse.json();
                const ephemeralKey = tokenData.token;
                console.log("Ephemeral key received from token service");

                // Mask the ephemeral key in the log message.
                logMessage("Ephemeral Key Received from Token Service: " + "***");

                // Set up the WebRTC connection using the ephemeral key.
                init(ephemeralKey);

            } catch (error) {
                console.error("Error fetching ephemeral key:", error);
                logMessage("Error fetching ephemeral key: " + error.message);
            }
        }

        async function init(ephemeralKey) {
            logMessage("🚀 Starting WebRTC initialization...");

            let peerConnection = new RTCPeerConnection();
            logMessage("✅ RTCPeerConnection created");

            // Set up to play remote audio from the model.
            const audioElement = document.createElement('audio');
            audioElement.autoplay = true;
            document.body.appendChild(audioElement);
            logMessage("🔊 Audio element created and added to page");

            peerConnection.ontrack = (event) => {
                logMessage("🎵 Remote track received! Type: " + event.track.kind);
                logMessage("📊 Number of streams: " + event.streams.length);

                if (event.streams.length > 0) {
                    audioElement.srcObject = event.streams[0];
                    logMessage("✅ Audio stream assigned to audio element");

                    // Add event listeners to audio element for debugging
                    audioElement.onloadstart = () => logMessage("🔄 Audio loading started");
                    audioElement.oncanplay = () => logMessage("▶️ Audio can start playing");
                    audioElement.onplay = () => logMessage("🎵 Audio playback started");
                    audioElement.onerror = (e) => logMessage("❌ Audio error: " + e.message);
                } else {
                    logMessage("⚠️ No streams in track event");
                }
            };

                // Set up data channel for sending and receiving events
            logMessage("🎤 Requesting microphone access...");
            try {
                const clientMedia = await navigator.mediaDevices.getUserMedia({ audio: true });
                logMessage("✅ Microphone access granted");

                const audioTrack = clientMedia.getAudioTracks()[0];
                logMessage("🎤 Audio track obtained: " + audioTrack.label);

                peerConnection.addTrack(audioTrack);
                logMessage("✅ Audio track added to peer connection");
            } catch (error) {
                logMessage("❌ Failed to get microphone access: " + error.message);
                return;
            }

            const dataChannel = peerConnection.createDataChannel('realtime-channel');
            logMessage("📡 Data channel created");

            dataChannel.addEventListener('open', () => {
                logMessage('✅ Data channel is open - ready to send messages');

                // Send client events to start the conversation
                logMessage("📝 Preparing to send text input message...");
                const event = {
                    type: "conversation.item.create",
                    item: {
                        type: "message",
                        role: "user",
                        content: [
                            {
                                type: "input_text",
                                text: "hello there! Can you give me some vacation options?",
                            },
                        ],
                    },
                };

                logMessage("📤 Sending conversation.item.create event...");
                logMessage("💬 Text content: " + event.item.content[0].text);

                try {
                    dataChannel.send(JSON.stringify(event));
                    logMessage("✅ Text input sent successfully!");

                    // Now send response.create to trigger the AI response
                    const responseEvent = {
                        type: "response.create"
                    };

                    logMessage("📤 Sending response.create event to trigger AI response...");
                    dataChannel.send(JSON.stringify(responseEvent));
                    logMessage("✅ Response.create sent successfully!");

                } catch (error) {
                    logMessage("❌ Failed to send text input: " + error.message);
                }
            });                dataChannel.addEventListener('message', (event) => {
                    const realtimeEvent = JSON.parse(event.data);
                    console.log(realtimeEvent);
                    logMessage("Received server event: " + JSON.stringify(realtimeEvent, null, 2));
                    if (realtimeEvent.type === "session.update") {
                        const instructions = realtimeEvent.session.instructions;
                        logMessage("Instructions: " + instructions);
                    } else if (realtimeEvent.type === "session.error") {
                        logMessage("Error: " + realtimeEvent.error.message);
                    } else if (realtimeEvent.type === "session.end") {
                        logMessage("Session ended.");
                    }
                });

                dataChannel.addEventListener('close', () => {
                    logMessage('Data channel is closed');
                });

            // Start the session using the Session Description Protocol (SDP)
            logMessage("🤝 Creating WebRTC offer...");
            const offer = await peerConnection.createOffer();
            await peerConnection.setLocalDescription(offer);
            logMessage("✅ Local description set");

            logMessage("📡 Sending SDP offer to: " + WEBRTC_URL);
            const sdpResponse = await fetch(`${WEBRTC_URL}`, {
                method: "POST",
                body: offer.sdp,
                headers: {
                    Authorization: `Bearer ${ephemeralKey}`,
                    "Content-Type": "application/sdp",
                },
            });

            logMessage("📥 Received SDP response, status: " + sdpResponse.status);
            if (!sdpResponse.ok) {
                logMessage("❌ SDP exchange failed: " + sdpResponse.statusText);
                return;
            }

            const answerSdp = await sdpResponse.text();
            logMessage("✅ Got SDP answer, length: " + answerSdp.length + " chars");

            const answer = { type: "answer", sdp: answerSdp };
            await peerConnection.setRemoteDescription(answer);
            logMessage("✅ Remote description set - WebRTC connection should be establishing...");

            // Add connection state logging
            peerConnection.onconnectionstatechange = () => {
                logMessage("🔗 Connection state: " + peerConnection.connectionState);
            };

            peerConnection.oniceconnectionstatechange = () => {
                logMessage("🧊 ICE connection state: " + peerConnection.iceConnectionState);
            };                const button = document.createElement('button');
                button.innerText = 'Close Session';
                button.onclick = stopSession;
                document.body.appendChild(button);



                function stopSession() {
                    if (dataChannel) dataChannel.close();
                    if (peerConnection) peerConnection.close();
                    peerConnection = null;
                    logMessage("Session closed.");
                }

            }

            function logMessage(message) {
                const logContainer = document.getElementById("logContainer");
                const p = document.createElement("p");
                p.textContent = message;
                logContainer.appendChild(p);
            }
        </script>
    </body>
    </html>
```

在示例中，我们使用查询参数 webrtcfilter=on。 此查询参数限制发送到浏览器的数据通道消息，以保持提示说明的私密性。 打开筛选器后，数据通道上仅返回以下消息到浏览器：

* input\_audio\_buffer.speech\_started (音频输入缓冲区.语音启动)
* input\_audio\_buffer.speech\_stopped
* 输出音频缓冲区.已启动
* output\_audio\_buffer.stopped
* 对话项.输入音频转录.已完成
* conversation.item.added
* 对话项目已创建
* response.output\_text.delta
* 响应.输出\_文本.完成
* response.output\_audio\_transcript.delta
* 响应.输出\_音频\_记录.完成

提示

有关实时 API 事件的完整列表，请参阅 [API 参考](../realtime-audio-reference)。

连接成功后，应会看到以下控制台消息：

* `✅ RTCPeerConnection created`
* `✅ Microphone access granted`
* `✅ Data channel is open`
* `🎵 Audio playback started`

如果 AI 响应，你将看到带有转录响应的 `response.output_audio_transcript.done` 事件。

**Reference：**[RTCPeerConnection](https://developer.mozilla.org/en-US/docs/Web/API/RTCPeerConnection)、 [Realtime API 事件](../realtime-audio-reference)

### 步骤 3 （可选）：创建 Websocket 观察程序/控制器

如果通过服务应用程序代理会话协商，则可以分析返回的 Location 标头，并使用该标头创建与 WebRTC 调用的 Websocket 连接。 此连接可以通过直接发出 session.update 事件和其他命令来记录 WebRTC 调用，甚至可以对其进行控制。

下面是前面显示的token\_service的更新版本，现在有一个 /connect 终结点，可用于获取临时令牌并协商会话启动。 它还包括侦听 WebRTC 会话的 Websocket 连接。

```
from flask import Flask, jsonify, request
#from flask_cors import CORS

import os
import requests
import time
import threading
import asyncio
import json
import websockets

from azure.identity import DefaultAzureCredential

app = Flask(__name__)
# CORS(app)  # Enable CORS for all routes when running locally for testing

# Session configuration
session_config = {
    "session": {
        "type": "realtime",
        "model": "<YOUR MODEL DEPLOYMENT NAME>",
        "instructions": "You are a helpful assistant.",
        "audio": {
            "output": {
                "voice": "marin",
            },
        },
    },
}

# Get configuration from environment variables
azure_resource = os.getenv('AZURE_RESOURCE')  # e.g., 'your-azure-resource'

# Token caching variables
cached_token = None
token_expiry = 0
token_lock = threading.Lock()

def get_bearer_token(resource_scope: str) -> str:
    """Get a bearer token using DefaultAzureCredential with caching."""
    global cached_token, token_expiry

    current_time = time.time()

    # Check if we have a valid cached token (with 5 minute buffer before expiry)
    with token_lock:
        if cached_token and current_time < (token_expiry - 300):
            return cached_token

    # Get a new token
    try:
        credential = DefaultAzureCredential()
        token = credential.get_token(resource_scope)

        with token_lock:
            cached_token = token.token
            token_expiry = token.expires_on

        print(f"Acquired new bearer token, expires at: {time.ctime(token_expiry)}")
        return cached_token

    except Exception as e:
        print(f"Failed to acquire bearer token: {e}")
        raise

def get_ephemeral_token():
    """
    Generate an ephemeral token from Azure OpenAI.

    Returns:
        str: The ephemeral token

    Raises:
        Exception: If token generation fails
    """
    # Get bearer token using DefaultAzureCredential
    bearer_token = get_bearer_token("https://ai.azure.com/.default")

    # Construct the Azure OpenAI endpoint URL
    url = f"https://{azure_resource}.openai.azure.com/openai/v1/realtime/client_secrets"

    headers = {
        "Authorization": f"Bearer {bearer_token}",
        "Content-Type": "application/json",
    }

    # Make the request to Azure OpenAI
    response = requests.post(
        url,
        headers=headers,
        json=session_config,
        timeout=30
    )

    # Check if the request was successful
    if response.status_code != 200:
        print(f"Request failed with status {response.status_code}: {response.reason}")
        print(f"Response headers: {dict(response.headers)}")
        print(f"Response content: {response.text}")

    response.raise_for_status()

    # Parse the JSON response and extract the ephemeral token
    data = response.json()
    ephemeral_token = data.get('value', '')

    if not ephemeral_token:
        print(f"No ephemeral token found in response: {data}")
        raise Exception("No ephemeral token available")

    return ephemeral_token

def perform_sdp_negotiation(ephemeral_token, sdp_offer):
    """
    Perform SDP negotiation with the Azure OpenAI Realtime API.

    Args:
        ephemeral_token (str): The ephemeral token for authentication
        sdp_offer (str): The SDP offer to send

    Returns:
        tuple: (sdp_answer, location_header) - The SDP answer from the server and Location header for WebSocket

    Raises:
        Exception: If SDP negotiation fails
    """
    # Construct the realtime endpoint URL - matching the v1transceiver_test pattern
    realtime_url = f"https://{azure_resource}.openai.azure.com/openai/v1/realtime/calls"

    headers = {
        'Authorization': f'Bearer {ephemeral_token}',
        'Content-Type': 'application/sdp'  # Azure OpenAI expects application/sdp, not form data
    }

    print(f"Sending SDP offer to: {realtime_url}")

    # Send the SDP offer as raw body data (not form data)
    response = requests.post(realtime_url, data=sdp_offer, headers=headers, timeout=30)

    if response.status_code == 201:  # Changed from 200 to 201 to match the test expectation
        sdp_answer = response.text
        location_header = response.headers.get('Location', '')
        print(f"Received SDP answer: {sdp_answer[:100]}...")
        if location_header:
            print(f"Captured Location header: {location_header}")
        else:
            print("Warning: No Location header found in response")
        return sdp_answer, location_header
    else:
        error_msg = f"SDP negotiation failed: {response.status_code} - {response.text}"
        print(error_msg)
        raise Exception(error_msg)

@app.route('/token', methods=['GET'])
def get_token():
    """
    An endpoint which returns an ephemeral token for Azure OpenAI Realtime API.
    Uses DefaultAzureCredential for authentication with token caching.
    """
    try:
        ephemeral_token = get_ephemeral_token()

        return jsonify({
            "token": ephemeral_token,
            "endpoint": f"https://{azure_resource}.openai.azure.com",
            "deployment": "gpt-4o-realtime-preview"
        })

    except requests.exceptions.RequestException as e:
        print(f"Token generation error: {e}")
        if hasattr(e, 'response') and e.response is not None:
            print(f"Response status: {e.response.status_code}")
            print(f"Response reason: {e.response.reason}")
            print(f"Response content: {e.response.text}")
        return jsonify({"error": "Failed to generate token"}), 500
    except Exception as e:
        print(f"Unexpected error: {e}")
        return jsonify({"error": "Failed to generate token"}), 500

async def connect_websocket(location_header, bearer_token=None, api_key=None):
    """
    Connect to the WebSocket endpoint using the Location header.
    Similar to the _connect_websocket function in run_v1transceiver_test.py

    Args:
        location_header (str): The Location header from the SDP negotiation response
        bearer_token (str, optional): Bearer token for authentication
        api_key (str, optional): API key for authentication (fallback)

    Returns:
        None: Just logs messages, doesn't store them
    """

    # Extract call_id from location header
    # Example: /v1/realtime/calls/rtc_abc123 -> rtc_abc123
    call_id = location_header.split('/')[-1]
    print(f"Extracted call_id: {call_id}")

    # Construct WebSocket URL: wss://<resource>.openai.azure.com/openai/v1/realtime?call_id=<call_id>
    ws_url = f"wss://{azure_resource}.openai.azure.com/openai/v1/realtime?call_id={call_id}"
    print(f"Connecting to WebSocket: {ws_url}")

    message_count = 0

    try:
        # WebSocket headers - use proper authentication
        headers = {}

        if bearer_token is not None:
            print("Using Bearer token for WebSocket authentication")
            headers["Authorization"] = f"Bearer {bearer_token}"
        elif api_key is not None:
            print("Using API key for WebSocket authentication")
            headers["api-key"] = api_key
        else:
            print("Warning: No authentication provided for WebSocket")

        async with websockets.connect(ws_url, additional_headers=headers) as websocket:
            print("WebSocket connection established")

            # Listen for messages
            try:
                async for message in websocket:
                    try:
                        # Parse JSON message
                        json_data = json.loads(message)
                        msg_type = json_data.get('type', 'unknown')
                        message_count += 1
                        print(f"WebSocket [{message_count}]: {msg_type}")

                        # Handle specific message types with additional details
                        if msg_type == 'response.done':
                            session_status = json_data['response'].get('status', 'unknown')
                            session_details = json_data['response'].get('details', 'No details provided')
                            print(f"  -> Response status: {session_status}, Details: {session_details}")
                            # Continue listening instead of breaking
                        elif msg_type == 'session.created':
                            session_id = json_data.get('session', {}).get('id', 'unknown')
                            print(f"  -> Session created: {session_id}")
                        elif msg_type == 'error':
                            error_message = json_data.get('error', {}).get('message', 'No error message')
                            print(f"  -> Error: {error_message}")

                    except json.JSONDecodeError:
                        message_count += 1
                        print(f"WebSocket [{message_count}]: Non-JSON message: {message[:100]}...")
                    except Exception as e:
                        print(f"Error processing WebSocket message: {e}")

            except websockets.exceptions.ConnectionClosed:
                print(f"WebSocket connection closed by remote (processed {message_count} messages)")
            except Exception as e:
                print(f"WebSocket message loop error: {e}")

    except Exception as e:
        print(f"WebSocket connection error: {e}")

    print(f"WebSocket monitoring completed. Total messages processed: {message_count}")

def start_websocket_background(location_header, bearer_token):
    """
    Start WebSocket connection in background thread to monitor/record the call.
    """
    def run_websocket():
        try:
            print(f"Starting background WebSocket monitoring for: {location_header}")

            # Create new event loop for this thread
            loop = asyncio.new_event_loop()
            asyncio.set_event_loop(loop)

            try:
                # Run the WebSocket connection (now just logs, doesn't return messages)
                loop.run_until_complete(
                    connect_websocket(location_header, bearer_token)
                )
                print("Background WebSocket monitoring completed.")

            except Exception as e:
                print(f"Background WebSocket error: {e}")
            finally:
                loop.close()

        except Exception as e:
            print(f"Failed to start background WebSocket: {e}")

    # Start the WebSocket in a background thread
    websocket_thread = threading.Thread(target=run_websocket, daemon=True)
    websocket_thread.start()
    print("Background WebSocket thread started")

@app.route('/connect', methods=['POST'])
def connect_and_negotiate():
    """
    Get token and perform SDP negotiation.
    Expects multipart form data with 'sdp' field containing the SDP offer.
    Returns SDP answer as plain text (matching the v1transceiver_test behavior).
    Automatically starts WebSocket connection in background to monitor/record the call.
    """
    try:
        # Get the SDP offer from multipart form data
        if 'sdp' not in request.form:
            return jsonify({"error": "Missing 'sdp' field in multipart form data"}), 400

        sdp_offer = request.form['sdp']
        print(f"Received SDP offer: {sdp_offer[:100]}...")

        # Get ephemeral token using shared function
        ephemeral_token = get_ephemeral_token()
        print(f"Got ephemeral token for SDP negotiation: {ephemeral_token[:20]}...")

        # Perform SDP negotiation using shared function
        sdp_answer, location_header = perform_sdp_negotiation(ephemeral_token, sdp_offer)

        # Create response headers
        response_headers = {'Content-Type': 'application/sdp'}

        # If we have a location header, start WebSocket connection in background to monitor/record the call
        if location_header:
            try:
                # Get a bearer token for WebSocket authentication
                bearer_token = get_bearer_token("https://ai.azure.com/.default")
                start_websocket_background(location_header, bearer_token)
            except Exception as e:
                print(f"Failed to start background WebSocket monitoring: {e}")
                # Don't fail the main request if WebSocket setup fails

        # Return SDP answer as plain text, just like the v1transceiver_test expects
        return sdp_answer, 201, response_headers

    except Exception as e:
        error_msg = f"Error in SDP negotiation: {e}"
        print(error_msg)
        return jsonify({"error": error_msg}), 500

if __name__ == '__main__':
    if not azure_resource:
        print("Error: AZURE_RESOURCE environment variable is required")
        exit(1)

    print(f"Starting token service for Azure resource: {azure_resource}")
    print("Using DefaultAzureCredential for authentication")

    port = int(os.getenv('PORT', 5000))
    print(f"  gunicorn -w 4 -b 0.0.0.0:{port} --timeout 30 token-service:app")
```

此处显示了关联的浏览器更改。

```
<!DOCTYPE html>
<html lang="en">
    <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>Azure OpenAI Realtime Session - Connect Endpoint</title>
    </head>
    <body>
        <h1>Azure OpenAI Realtime Session - Using /connect Endpoint</h1>
        <button onclick="StartSession()">Start Session</button>

        <!-- Log container for API messages -->
        <div id="logContainer"></div>

        <script>

        const AZURE_RESOURCE = "YOUR AZURE RESOURCE NAME"

        async function StartSession() {
            try {
                logMessage("🚀 Starting session with /connect endpoint...");

                // Set up the WebRTC connection first
                const peerConnection = new RTCPeerConnection();
                logMessage("✅ RTCPeerConnection created");

                // Get microphone access and add audio track BEFORE creating offer
                logMessage("🎤 Requesting microphone access...");
                try {
                    const clientMedia = await navigator.mediaDevices.getUserMedia({ audio: true });
                    logMessage("✅ Microphone access granted");

                    const audioTrack = clientMedia.getAudioTracks()[0];
                    logMessage("🎤 Audio track obtained: " + audioTrack.label);

                    peerConnection.addTrack(audioTrack);
                    logMessage("✅ Audio track added to peer connection");
                } catch (error) {
                    logMessage("❌ Failed to get microphone access: " + error.message);
                    return;
                }

                // Set up audio playback
                const audioElement = document.createElement('audio');
                audioElement.autoplay = true;
                document.body.appendChild(audioElement);
                logMessage("🔊 Audio element created and added to page");

                peerConnection.ontrack = (event) => {
                    logMessage("🎵 Remote track received! Type: " + event.track.kind);
                    logMessage("📊 Number of streams: " + event.streams.length);

                    if (event.streams.length > 0) {
                        audioElement.srcObject = event.streams[0];
                        logMessage("✅ Audio stream assigned to audio element");

                        // Add event listeners to audio element for debugging
                        audioElement.onloadstart = () => logMessage("🔄 Audio loading started");
                        audioElement.oncanplay = () => logMessage("▶️ Audio can start playing");
                        audioElement.onplay = () => logMessage("🎵 Audio playback started");
                        audioElement.onerror = (e) => logMessage("❌ Audio error: " + e.message);
                    } else {
                        logMessage("⚠️ No streams in track event");
                    }
                };

                // Set up data channel BEFORE SDP exchange
                const dataChannel = peerConnection.createDataChannel('realtime-channel');
                logMessage("📡 Data channel created");

                dataChannel.addEventListener('open', () => {
                    logMessage('✅ Data channel is open - ready to send messages');

                    // Send client events to start the conversation
                    logMessage("📝 Preparing to send text input message...");
                    const event = {
                        type: "conversation.item.create",
                        item: {
                            type: "message",
                            role: "user",
                            content: [
                                {
                                    type: "input_text",
                                    text: "hello there! Can you give me some vacation options?",
                                },
                            ],
                        },
                    };

                    logMessage("📤 Sending conversation.item.create event...");
                    logMessage("💬 Text content: " + event.item.content[0].text);

                    try {
                        dataChannel.send(JSON.stringify(event));
                        logMessage("✅ Text input sent successfully!");

                        // Now send response.create to trigger the AI response
                        const responseEvent = {
                            type: "response.create"
                        };

                        logMessage("📤 Sending response.create event to trigger AI response...");
                        dataChannel.send(JSON.stringify(responseEvent));
                        logMessage("✅ Response.create sent successfully!");

                    } catch (error) {
                        logMessage("❌ Failed to send text input: " + error.message);
                    }
                });

                dataChannel.addEventListener('message', (event) => {
                    const realtimeEvent = JSON.parse(event.data);
                    console.log(realtimeEvent);
                    logMessage("📥 Received server event: " + realtimeEvent.type);

                    // Log more detail for important events
                    if (realtimeEvent.type === "error") {
                        logMessage("❌ Error: " + realtimeEvent.error.message);
                    } else if (realtimeEvent.type === "session.created") {
                        logMessage("🎉 Session created successfully");
                    } else if (realtimeEvent.type === "response.output_audio_transcript.done") {
                        logMessage("📝 AI transcript complete: " + (realtimeEvent.transcript || ""));
                    } else if (realtimeEvent.type === "response.done") {
                        logMessage("✅ Response completed");
                    }
                });

                dataChannel.addEventListener('close', () => {
                    logMessage('❌ Data channel is closed');
                });

                dataChannel.addEventListener('error', (error) => {
                    logMessage('❌ Data channel error: ' + error);
                });

                // Add connection state logging
                peerConnection.onconnectionstatechange = () => {
                    logMessage("🔗 Connection state: " + peerConnection.connectionState);
                };

                peerConnection.oniceconnectionstatechange = () => {
                    logMessage("🧊 ICE connection state: " + peerConnection.iceConnectionState);
                };

                // Create offer AFTER setting up data channel
                const offer = await peerConnection.createOffer();
                await peerConnection.setLocalDescription(offer);
                logMessage("🤝 WebRTC offer created with audio track");

                // Prepare multipart form data for /connect endpoint
                const formData = new FormData();
                formData.append('sdp', offer.sdp);

                logMessage("📤 Sending SDP via multipart form to /connect endpoint...");

                // Call our /connect endpoint with multipart form data
                const connectResponse = await fetch("/connect", {
                    method: "POST",
                    body: formData  // FormData automatically sets correct Content-Type
                });

                if (!connectResponse.ok) {
                    throw new Error(`Connect service request failed: ${connectResponse.status}`);
                }

                // Get the SDP answer directly as text (not JSON)
                const answerSdp = await connectResponse.text();
                logMessage("✅ Got SDP answer from /connect endpoint, length: " + answerSdp.length + " chars");

                // Set up the WebRTC connection using the SDP answer
                const answer = { type: "answer", sdp: answerSdp };
                await peerConnection.setRemoteDescription(answer);
                logMessage("✅ Remote description set");

                // Add close session button
                const button = document.createElement('button');
                button.innerText = 'Close Session';
                button.onclick = () => stopSession(dataChannel, peerConnection);
                document.body.appendChild(button);
                logMessage("🔴 Close session button added");

                function stopSession(dataChannel, peerConnection) {
                    if (dataChannel) dataChannel.close();
                    if (peerConnection) peerConnection.close();
                    logMessage("Session closed.");
                }

            } catch (error) {
                console.error("Error in StartSession:", error);
                logMessage("Error in StartSession: " + error.message);
            }
        }

        function logMessage(message) {
            const logContainer = document.getElementById("logContainer");
            const p = document.createElement("p");
            p.textContent = message;
            logContainer.appendChild(p);
        }

        async function init(peerConnection) {
            logMessage("� Continuing WebRTC setup with existing peer connection...");

            // Set up to play remote audio from the model.
            const audioElement = document.createElement('audio');
            audioElement.autoplay = true;
            document.body.appendChild(audioElement);
            logMessage("🔊 Audio element created and added to page");

            peerConnection.ontrack = (event) => {
                logMessage("🎵 Remote track received! Type: " + event.track.kind);
                logMessage("📊 Number of streams: " + event.streams.length);

                if (event.streams.length > 0) {
                    audioElement.srcObject = event.streams[0];
                    logMessage("✅ Audio stream assigned to audio element");

                    // Add event listeners to audio element for debugging
                    audioElement.onloadstart = () => logMessage("🔄 Audio loading started");
                    audioElement.oncanplay = () => logMessage("▶️ Audio can start playing");
                    audioElement.onplay = () => logMessage("🎵 Audio playback started");
                    audioElement.onerror = (e) => logMessage("❌ Audio error: " + e.message);
                } else {
                    logMessage("⚠️ No streams in track event");
                }
            };

            const dataChannel = peerConnection.createDataChannel('realtime-channel');
            logMessage("📡 Data channel created");

            dataChannel.addEventListener('open', () => {
                logMessage('✅ Data channel is open - ready to send messages');

                // Send client events to start the conversation
                logMessage("📝 Preparing to send text input message...");
                const event = {
                    type: "conversation.item.create",
                    item: {
                        type: "message",
                        role: "user",
                        content: [
                            {
                                type: "input_text",
                                text: "hello there! Can you give me some vacation options?",
                            },
                        ],
                    },
                };

                logMessage("📤 Sending conversation.item.create event...");
                logMessage("💬 Text content: " + event.item.content[0].text);

                try {
                    dataChannel.send(JSON.stringify(event));
                    logMessage("✅ Text input sent successfully!");

                    // Now send response.create to trigger the AI response
                    const responseEvent = {
                        type: "response.create"
                    };

                    logMessage("📤 Sending response.create event to trigger AI response...");
                    dataChannel.send(JSON.stringify(responseEvent));
                    logMessage("✅ Response.create sent successfully!");

                } catch (error) {
                    logMessage("❌ Failed to send text input: " + error.message);
                }
            });

            dataChannel.addEventListener('close', () => {
                logMessage('❌ Data channel is closed');
            });

            dataChannel.addEventListener('error', (error) => {
                logMessage('❌ Data channel error: ' + error);
            });            // Add connection state logging
            peerConnection.onconnectionstatechange = () => {
                logMessage("� Connection state: " + peerConnection.connectionState);
            };

            peerConnection.oniceconnectionstatechange = () => {
                logMessage("🧊 ICE connection state: " + peerConnection.iceConnectionState);
            };

            // Add close session button
            const button = document.createElement('button');
            button.innerText = 'Close Session';
            button.onclick = () => stopSession(dataChannel, peerConnection);
            document.body.appendChild(button);
            logMessage("🔴 Close session button added");

            function stopSession(dataChannel, peerConnection) {
                if (dataChannel) dataChannel.close();
                if (peerConnection) peerConnection.close();
                logMessage("Session closed.");
            }
        }

            function logMessage(message) {
                const logContainer = document.getElementById("logContainer");
                const p = document.createElement("p");
                p.textContent = message;
                logContainer.appendChild(p);
            }
        </script>
    </body>
</html>
```

**参考：**[DefaultAzureCredential](/zh-cn/python/api/azure-identity/azure.identity.defaultazurecredential)、 [Flask 文档](https://flask.palletsprojects.com/)

### 步骤 4（可选）：配置网络防火墙

如果您使用网络防火墙，客户端实时 API 需要以下 `Allow` 规则。

| 港口 | 协议 | 规则 | IP 范围 |
| --- | --- | --- | --- |
| 3478 | UDP、TCP | 允许 | 属于 `AzureCloud.<Your_Microsoft_Foundry_Resource_Azure_Region>`[服务标记](/zh-cn/azure/virtual-network/service-tags-overview)的所有子网。 示例 `AzureCloud.eastus2`。 [查看Azure IP 范围和服务标记的完整列表](https://www.microsoft.com/download/details.aspx?id=56519)。 |

## 故障 排除

### 身份验证错误

* **401 未授权**：验证 API 密钥或Microsoft Entra ID令牌是否有效。 确保标识在 Azure OpenAI 资源中被指派了**认知服务用户**角色。
* **403 禁止访问**：检查资源是否部署在受支持的区域（美国东部 2 或瑞典中部）。

### 连接错误

* **WebRTC 连接失败**：
  + 确保浏览器支持 WebRTC 并允许麦克风访问。 检查您是否使用 HTTPS （这对 `getUserMedia` 是必需的）。
  + 如果使用网络防火墙，请检查 [防火墙设置](#step-4-optional-configure-network-firewall)。
* **数据通道未打开**：检查浏览器控制台是否存在 ICE 连接状态错误。 检查临时令牌是否已过期。
* **SDP 交换失败**：验证 WebRTC 终结点 URL 是否正确，临时令牌有效。

### 模型错误

* **找不到模型**：验证部署名称是否完全匹配（区分大小写）。 确保已部署实时模型（`gpt-4o-realtime-preview``gpt-realtime`等）。
* **超出配额**：检查 Azure 门户中的 Azure OpenAI 配额。 实时 API 拥有与聊天补全独立的配额。

### 音频问题

* **无音频输出**：检查是否已 `audioElement.autoplay = true` 设置，浏览器自动播放策略不会阻止播放。 请尝试先单击页面以启用音频。
* **音频质量不佳**：WebRTC 会自动根据网络条件进行调整。 检查网络连接并尝试减少其他网络流量。

## 相关内容

* 请尝试 [实时音频快速入门](about:blank/realtime-audio-websockets#voice-agent-quickstart)。
* 请参阅 [实时 API 参考](../realtime-audio-reference)
* 详细了解 Azure OpenAI [配额和限制](../quotas-limits)
