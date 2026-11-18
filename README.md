# DeepSeek 功能全家桶（deepseek-tools）

Miyu 的扩展包：把 [DeepSeek 的接口](https://api-docs.deepseek.com) 和状态页接进 Miyu。

- **状态查询**：AI 用的工具 `deepseek_status`，查 DeepSeek 官方状态页（<https://status.deepseek.com/>）：各项服务现在好不好、
  近 30 天可用率，正在处理的和最近的故障。不用 key。照 miyu-scripts 的 `deepseek-status` 脚本搬成 Rust，参数、输出一样，
  Windows 上不用装 Python。
- **余额查询**：工具 `deepseek_balance`，查 DeepSeek 账户能不能调接口、每种货币的总余额、赠送的、充值的。
- **语音合成**接进总线的 `tts.engine`：朗读（`say`）的配置里「引擎」选「DeepSeek 功能全家桶」，就用它念。两个音色：`stella`（海星，女声，默认）、
  `tide`（暗潮，男声）。
- **语音识别**接进总线的 `stt.engine`：语音输入（`voice-input`）的配置里「引擎」选「DeepSeek 功能全家桶」，就用它听写，不用下本机 163 MB 的模型。
- **文字识别**：工具 `deepseek_ocr`，认 PDF、Word（DOC、DOCX）、PowerPoint（PPT、PPTX）、图片里的字，逐页交回。

```sh
miyu pkg install deepseek-tools
miyu deepseek-tools               # 看 key 有没有、怎么用
miyu deepseek-tools ocr 合同.pdf   # 在命令行认一个文件
miyu deepseek-tools ocr 海报.png --layout
```

**文件、声音都会传到 DeepSeek**，照 DeepSeek 的价钱收钱：合成照字数，识别照时长，OCR 照页数（认出来的头一行会写几页）。

## key

用你在 Miyu 里给 DeepSeek 供应商配的 key：装的时候确认屏会写「这个包会拿到你 DeepSeek 的 key」，点了才给。key 改了、删了，
核心会推过来，跟着换（`src/credentials.rs`，全在一处）。这个 key 只发到 `https://api.deepseek.com`。

没配 key 时：合成、识别不接进总线，`deepseek_balance`、`deepseek_ocr` 回「还没有 key」，什么都不发；
`deepseek_status` 不用 key，照样能用。

开发时可以用两个测试钩子：`MIYU_DEEPSEEK_TEST_KEY`（key）、`MIYU_DEEPSEEK_TEST_BASE_URL`（接口地址，不写是
`https://api.deepseek.com`）。设了测试 key 的用测试 key；测试地址只跟测试 key 走，改不了你的 key 发去哪儿。

## 怎么接的

- 合成：`POST /v1/tools/tts`，要 WAV。DeepSeek 只出 WAV 和裸 PCM，朗读要 MP3 时也交 WAV、写实 `audio/wav`；不能调语速，
  `speed` 收下不用。音频边合成边发，头里的长度交出去之前重写；中途断了的当失败。
- 识别：`POST /v1/tools/asr`，multipart 的 `file`。DeepSeek 一次至多 8 MiB、120 秒，长过 110 秒的录音按帧切成几段，一段段识别，
  字接起来。
- OCR：`POST /v1/tools/ocr`，multipart 的 `purpose`、`file`；至多 100 MiB，传之前先看大小，文件边读边发。认的字里带着定位记号
  （`<｜ref｜>`、`<｜box｜>` 和坐标），给 AI 看的去掉，原样的回答留在 `data` 里给脚本。
- 余额：`GET /user/balance`（不在 `/v1` 底下），原样的回答在 `data` 里。
- 状态页：取 <https://status.deepseek.com/> 那一页，从 Next.js 的数据段里读服务、可用率、故障。`tests/fixtures/status/` 里是一份真的页面和
  原来的 Python 脚本对它的输出，测试照着对答案，一字不差。
- 出错照 DeepSeek 文档的状态码说一句英文，带上 DeepSeek 自己的原话；key 只在请求头里，报错、日志里不出现。
- 连接走系统的代理设置（`HTTPS_PROXY`、`NO_PROXY`）。

## 开发

```sh
cargo test --workspace   # 不连真的 DeepSeek：测试在本机起一个假的
cargo xtask licenses     # 第三方许可证合不合、THIRD-PARTY-LICENSES.txt 是不是最新的
cargo xtask notices      # 升级依赖以后重新生成清单
```

打标签 `v<版本>` 发版：三种系统各打一个 `.miyu`（`.github/workflows/release.yml`）。

## 许可证

AGPL-3.0-or-later。第三方依赖的许可证见 `THIRD-PARTY-LICENSES.txt`。
