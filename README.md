# DeepSeek 功能全家桶（deepseek-tools）

DeepSeek 功能全家桶是 Miyu 的扩展包，它把 [DeepSeek 的接口](https://api-docs.deepseek.com)和 DeepSeek 的状态页接进 Miyu。安装之后，你可以使用下面这些功能：

- **状态查询**：AI 可以用工具 `deepseek_status` 查看 DeepSeek 官方状态页（<https://status.deepseek.com/>），了解各项服务现在是否正常、近 30 天的可用率，以及正在处理和最近发生的故障。这个功能不需要 key。由于它是用 Rust 写的，Windows 上也不需要安装 Python。
- **余额查询**：AI 可以用工具 `deepseek_balance` 查看你的 DeepSeek 账户能否调用接口，以及每种货币的总余额、赠送余额和充值余额。
- **语音合成**：这个包可以作为朗读（`say`）的引擎。在朗读的设置里把 `引擎` 选为 `DeepSeek 功能全家桶`，朗读就会使用 DeepSeek 的语音合成。可以选择两个音色：`stella`（海星，女声，默认）和 `tide`（暗潮，男声）。
- **语音识别**：这个包可以作为语音输入（`voice-input`）的引擎。在语音输入的设置里把 `引擎` 选为 `DeepSeek 功能全家桶`，语音输入就会使用 DeepSeek 的语音识别，因此不需要下载本机 163 MB 的识别模型。
- **文字识别（OCR）**：AI 可以用工具 `deepseek_ocr` 识别 PDF、Word（DOC、DOCX）、PowerPoint（PPT、PPTX）和图片里的文字，结果按页返回。

安装和使用：

```sh
miyu pkg install deepseek-tools
miyu deepseek-tools               # 查看 key 是否已经设置，以及用法
miyu deepseek-tools ocr 合同.pdf   # 在命令行识别一个文件里的文字
miyu deepseek-tools ocr 海报.png --layout
```

> **你的文件和声音会上传到 DeepSeek，并且按 DeepSeek 的价格收费。** 语音合成按字数收费，语音识别按时长收费，文字识别按页数收费。文字识别结果的第一行会写明一共识别了几页。

## 设置 key

这个包使用你在 Miyu 里为 DeepSeek 模型供应商配置的 key。安装时，确认界面会显示 `这个包会拿到你 DeepSeek 的 key`，你确认之后包才会拿到 key。如果你之后修改或删除了这个 key，核心会把变化推送给这个包，包会随之更换。这个 key 只会发送到 `https://api.deepseek.com`。

如果还没有配置 key：

- 语音合成和语音识别无法使用。朗读和语音输入的引擎选项里仍然会列出 `DeepSeek 功能全家桶`，但是会标注 `没在运行`；
- `deepseek_balance` 和 `deepseek_ocr` 会回复 `No DeepSeek key yet…`，不会发出任何请求；
- `deepseek_status` 不需要 key，可以照常使用。

> 开发时可以使用两个测试用的环境变量：`MIYU_DEEPSEEK_TEST_KEY` 设置 key，`MIYU_DEEPSEEK_TEST_BASE_URL` 设置接口地址（没有设置时是 `https://api.deepseek.com`）。设置了测试 key 时，包会优先使用测试 key。测试地址只对测试 key 生效，因此它不能改变你自己的 key 的发送地址。

## 实现方式

这一节写给想了解或修改这个包的开发者。

- **语音合成**：调用 `POST /v1/tools/tts`，请求 WAV 格式。因为 DeepSeek 只提供 WAV 和裸 PCM 两种格式，所以朗读要求 MP3 时，这个包也交回 WAV，并如实标明 `audio/wav`。DeepSeek 不支持调整语速，因此收到的 `speed` 参数不起作用。音频边合成边接收，交出之前会重写 WAV 头里的长度；如果传输中途断开，这次合成按失败处理。
- **语音识别**：调用 `POST /v1/tools/asr`，用 multipart 的 `file` 字段上传。DeepSeek 一次最多接受 8 MiB、120 秒的音频，所以长于 110 秒的录音会按音频帧切成几段，逐段识别后再把文字连起来。
- **文字识别**：调用 `POST /v1/tools/ocr`，用 multipart 的 `purpose` 和 `file` 字段上传。文件最大 100 MiB，包会在上传之前先检查大小，上传时边读边发；如果文件在上传过程中变短，这次识别按失败处理。DeepSeek 返回的文字里带有定位记号（`<｜ref｜>`、`<｜box｜>` 和坐标），交给 AI 的文字会去掉这些记号，原始回答完整保留在 `data` 里供脚本使用。命令行使用的方法 `ocr` 只接受你在终端里运行的命令，其他扩展包不能调用它。
- **余额查询**：调用 `GET /user/balance`（这个地址不在 `/v1` 下面），原始回答保留在 `data` 里。
- **状态查询**：读取 <https://status.deepseek.com/> 页面，从页面里 Next.js 的数据段解析服务状态、可用率和故障。`tests/fixtures/status/` 里保存了一份真实的页面和对应的标准输出，测试会逐字核对。
- **报错**：出错时，包会根据 DeepSeek 文档里的状态码给出一句英文说明，并附上 DeepSeek 返回的原文。key 只放在请求头里，不会出现在报错和日志中。
- **代理**：网络连接使用系统的代理设置（`HTTPS_PROXY`、`NO_PROXY`）。

## 开发

```sh
cargo test --workspace   # 测试会在本机启动一个假的 DeepSeek 服务，不连接真实的 DeepSeek
cargo xtask licenses     # 检查第三方许可证是否兼容，以及 THIRD-PARTY-LICENSES.txt 是否是最新的
cargo xtask notices      # 升级依赖之后重新生成许可证清单
```

发版时打标签 `v<版本号>`，CI 会为三种系统各打一个 `.miyu` 包（见 `.github/workflows/release.yml`）。

## 许可证

AGPL-3.0-or-later。第三方依赖的许可证见 `THIRD-PARTY-LICENSES.txt`。
